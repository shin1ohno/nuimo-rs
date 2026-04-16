use std::sync::Arc;

use bluer::Adapter;
use bluer::Address;
use tokio::sync::{broadcast, mpsc, Mutex};

use crate::error::NuimoError;
use crate::event::NuimoEvent;
use crate::gatt;
use crate::glyph::{DisplayOptions, Glyph};
use crate::peripheral::NuimoPeripheral;

/// Rotation mode for the dial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationMode {
    /// Rotation clamped between min and max values.
    Clamped,
    /// Continuous rotation with unbounded delta.
    Continuous,
}

/// High-level interface to a Nuimo device.
pub struct NuimoDevice {
    address: Address,
    adapter_name: String,
    peripheral: Arc<Mutex<Option<NuimoPeripheral>>>,
    event_tx: broadcast::Sender<NuimoEvent>,
    rotation_mode: Arc<Mutex<RotationMode>>,
    rotation_state: Arc<Mutex<RotationState>>,
    battery: Arc<Mutex<Option<u8>>>,
    rssi_val: Arc<Mutex<Option<i16>>>,
}

struct RotationState {
    value: f64,
    min: f64,
    max: f64,
    cycles: f64,
}

impl Default for RotationState {
    fn default() -> Self {
        Self {
            value: 0.0,
            min: 0.0,
            max: 1.0,
            cycles: 1.0,
        }
    }
}

impl NuimoDevice {
    /// Create a new device handle (not yet connected).
    pub fn new(address: Address, adapter_name: &str) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            address,
            adapter_name: adapter_name.to_string(),
            peripheral: Arc::new(Mutex::new(None)),
            event_tx,
            rotation_mode: Arc::new(Mutex::new(RotationMode::Continuous)),
            rotation_state: Arc::new(Mutex::new(RotationState::default())),
            battery: Arc::new(Mutex::new(None)),
            rssi_val: Arc::new(Mutex::new(None)),
        }
    }

    /// Connect to the Nuimo device.
    pub async fn connect(&self) -> Result<(), NuimoError> {
        let session = bluer::Session::new()
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))?;
        let adapter = session
            .adapter(&self.adapter_name)
            .map_err(|e| NuimoError::Ble(e.to_string()))?;

        let (raw_tx, mut raw_rx) = mpsc::channel::<NuimoEvent>(64);
        let periph = NuimoPeripheral::connect(&adapter, self.address, raw_tx).await?;
        *self.peripheral.lock().await = Some(periph);

        // Forward raw events with rotation processing
        let event_tx = self.event_tx.clone();
        let rotation_mode = self.rotation_mode.clone();
        let rotation_state = self.rotation_state.clone();
        let battery = self.battery.clone();
        let rssi_val = self.rssi_val.clone();

        tokio::spawn(async move {
            while let Some(event) = raw_rx.recv().await {
                let processed = match event {
                    NuimoEvent::Rotate { delta, .. } => {
                        let mode = *rotation_mode.lock().await;
                        match mode {
                            RotationMode::Continuous => {
                                NuimoEvent::Rotate {
                                    delta,
                                    rotation: 0.0,
                                }
                            }
                            RotationMode::Clamped => {
                                let mut state = rotation_state.lock().await;
                                let range = state.max - state.min;
                                let cycle_delta = delta * range / state.cycles;
                                state.value = (state.value + cycle_delta)
                                    .clamp(state.min, state.max);
                                NuimoEvent::Rotate {
                                    delta,
                                    rotation: state.value,
                                }
                            }
                        }
                    }
                    NuimoEvent::BatteryLevel(level) => {
                        *battery.lock().await = Some(level);
                        event
                    }
                    NuimoEvent::Rssi(val) => {
                        *rssi_val.lock().await = Some(val);
                        event
                    }
                    other => other,
                };
                if event_tx.send(processed).is_err() {
                    break;
                }
            }
        });

        Ok(())
    }

    /// Disconnect from the device.
    pub async fn disconnect(&self) {
        if let Some(periph) = self.peripheral.lock().await.as_ref() {
            let _ = periph.disconnect().await;
        }
        *self.peripheral.lock().await = None;
    }

    /// Check if connected.
    pub async fn is_connected(&self) -> bool {
        if let Some(periph) = self.peripheral.lock().await.as_ref() {
            periph.is_connected().await
        } else {
            false
        }
    }

    /// Device BLE address.
    pub fn address(&self) -> Address {
        self.address
    }

    /// Device ID string.
    pub fn id(&self) -> String {
        self.address.to_string()
    }

    /// Subscribe to device events.
    pub fn events(&self) -> broadcast::Receiver<NuimoEvent> {
        self.event_tx.subscribe()
    }

    /// Current battery level (0-100).
    pub async fn battery_level(&self) -> Option<u8> {
        *self.battery.lock().await
    }

    /// Current RSSI.
    pub async fn rssi(&self) -> Option<i16> {
        if let Some(periph) = self.peripheral.lock().await.as_ref() {
            periph.rssi().await
        } else {
            None
        }
    }

    /// Display a glyph on the LED matrix.
    pub async fn display_glyph(
        &self,
        glyph: &Glyph,
        opts: &DisplayOptions,
    ) -> Result<(), NuimoError> {
        let periph = self.peripheral.lock().await;
        let periph = periph.as_ref().ok_or(NuimoError::NotConnected)?;
        let data = glyph.to_display_bytes(opts);
        periph.write_display(&data).await
    }

    /// Clear the display.
    pub async fn clear_display(&self) -> Result<(), NuimoError> {
        let empty = Glyph::empty();
        self.display_glyph(&empty, &DisplayOptions {
            brightness: 0.0,
            timeout_ms: 0,
            ..Default::default()
        }).await
    }

    /// Set rotation mode.
    pub async fn set_rotation_mode(&self, mode: RotationMode) {
        *self.rotation_mode.lock().await = mode;
    }

    /// Set rotation range (Clamped mode only).
    pub async fn set_rotation_range(&self, min: f64, max: f64, value: f64, cycles: f64) {
        let mut state = self.rotation_state.lock().await;
        state.min = min;
        state.max = max;
        state.value = value.clamp(min, max);
        state.cycles = cycles;
    }

    /// Current rotation value (Clamped mode only).
    pub async fn rotation(&self) -> f64 {
        self.rotation_state.lock().await.value
    }
}
