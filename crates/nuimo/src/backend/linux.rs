//! Linux BLE backend (bluer / BlueZ D-Bus). Formerly
//! `src/peripheral.rs` + the bluer bits of `src/discovery.rs`; identical
//! behaviour, just moved behind a cfg wall so the macOS build can link
//! `btleplug` instead.

use std::time::Duration;

use bluer::gatt::remote::Characteristic;
use bluer::{Adapter, AdapterEvent, Address, Device, DeviceEvent, DeviceProperty};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::error::NuimoError;
use crate::event::{parse_fly, parse_touch_or_swipe, NuimoEvent};
use crate::gatt;

/// A discovered Nuimo device (not yet connected).
#[derive(Debug, Clone)]
pub struct DiscoveredNuimo {
    /// Platform-neutral device identifier as a `String`.
    /// - Linux: `"XX:XX:XX:XX:XX:XX"` (BLE MAC)
    /// - macOS: CoreBluetooth peripheral UUID
    pub address: String,
    pub name: String,
    /// Hint for the host adapter (BlueZ adapter name on Linux, unused on macOS).
    pub adapter: String,
}

/// Scan for Nuimo devices via BLE. Yields `DiscoveredNuimo` values through
/// the returned `Receiver`; duplicates are filtered upstream by address.
pub async fn discover(
) -> Result<(mpsc::Receiver<DiscoveredNuimo>, tokio::task::JoinHandle<()>), NuimoError> {
    let session = bluer::Session::new()
        .await
        .map_err(|e| NuimoError::Ble(e.to_string()))?;
    let adapter = session
        .default_adapter()
        .await
        .map_err(|e| NuimoError::Ble(e.to_string()))?;
    adapter
        .set_powered(true)
        .await
        .map_err(|e| NuimoError::Ble(e.to_string()))?;

    let (tx, rx) = mpsc::channel::<DiscoveredNuimo>(16);
    let adapter_name = adapter.name().to_string();

    let handle = tokio::spawn(async move {
        // Keep _session alive so the adapter's D-Bus connection persists.
        let _session = session;

        // Poll BlueZ-cached devices for ones that existed before scanning
        // started, or that flap.
        let cache_adapter = adapter.clone();
        let cache_tx = tx.clone();
        let cache_adapter_name = adapter_name.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if let Ok(addrs) = cache_adapter.device_addresses().await {
                    for addr in addrs {
                        if let Ok(device) = cache_adapter.device(addr) {
                            report_if_nuimo(&device, addr, &cache_adapter_name, &cache_tx).await;
                        }
                    }
                }
            }
        });

        loop {
            if let Err(e) = scan_loop(&adapter, &tx, &adapter_name).await {
                tracing::warn!("Discovery scan ended: {}, restarting...", e);
            } else {
                tracing::info!("Discovery scan stream ended, restarting...");
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });

    Ok((rx, handle))
}

async fn scan_loop(
    adapter: &Adapter,
    tx: &mpsc::Sender<DiscoveredNuimo>,
    adapter_name: &str,
) -> Result<(), NuimoError> {
    if let Ok(addrs) = adapter.device_addresses().await {
        for addr in addrs {
            if let Ok(device) = adapter.device(addr) {
                report_if_nuimo(&device, addr, adapter_name, tx).await;
            }
        }
    }

    let discover = adapter
        .discover_devices()
        .await
        .map_err(|e| NuimoError::Ble(e.to_string()))?;
    tokio::pin!(discover);

    while let Some(event) = discover.next().await {
        if let AdapterEvent::DeviceAdded(addr) = event {
            let device = match adapter.device(addr) {
                Ok(d) => d,
                Err(_) => continue,
            };
            report_if_nuimo(&device, addr, adapter_name, tx).await;
        }
    }

    Ok(())
}

async fn report_if_nuimo(
    device: &Device,
    addr: Address,
    adapter_name: &str,
    tx: &mpsc::Sender<DiscoveredNuimo>,
) {
    if !is_nuimo(device).await {
        return;
    }
    let name = device
        .name()
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| gatt::DEVICE_NAME.to_string());
    let discovered = DiscoveredNuimo {
        address: addr.to_string(),
        name,
        adapter: adapter_name.to_string(),
    };
    tracing::info!("Discovered Nuimo: {} ({})", discovered.name, addr);
    let _ = tx.send(discovered).await;
}

async fn is_nuimo(device: &Device) -> bool {
    matches!(device.name().await, Ok(Some(ref name)) if name == gatt::DEVICE_NAME)
}

/// Low-level BLE peripheral handle.
pub struct NuimoPeripheral {
    _session: bluer::Session,
    device: Device,
    led_char: Option<Characteristic>,
    event_tx: mpsc::Sender<NuimoEvent>,
}

impl NuimoPeripheral {
    pub async fn connect(
        id: &str,
        adapter_hint: Option<&str>,
        event_tx: mpsc::Sender<NuimoEvent>,
    ) -> Result<Self, NuimoError> {
        let session = bluer::Session::new()
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))?;
        let adapter = match adapter_hint {
            Some(name) => session
                .adapter(name)
                .map_err(|e| NuimoError::Ble(e.to_string()))?,
            None => session
                .default_adapter()
                .await
                .map_err(|e| NuimoError::Ble(e.to_string()))?,
        };
        let address: Address = id
            .parse()
            .map_err(|_| NuimoError::Ble(format!("invalid BLE address: {id}")))?;

        let device = adapter
            .device(address)
            .map_err(|e| NuimoError::Ble(e.to_string()))?;

        tokio::time::timeout(
            Duration::from_secs(gatt::CONNECT_TIMEOUT_SECS),
            device.connect(),
        )
        .await
        .map_err(|_| NuimoError::ConnectionTimeout(gatt::CONNECT_TIMEOUT_SECS))?
        .map_err(|e| NuimoError::Ble(e.to_string()))?;

        tracing::info!("Connected to Nuimo {}", address);

        let mut led_char = None;
        let mut battery_char = None;
        let mut button_char = None;
        let mut rotation_char = None;
        let mut touch_char = None;
        let mut fly_char = None;

        let services = device
            .services()
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))?;

        for service in services {
            let service_uuid = service
                .uuid()
                .await
                .map_err(|e| NuimoError::Ble(e.to_string()))?;
            tracing::debug!("Service: {}", service_uuid);
            let chars = service
                .characteristics()
                .await
                .map_err(|e| NuimoError::Ble(e.to_string()))?;

            for char in chars {
                let uuid = char
                    .uuid()
                    .await
                    .map_err(|e| NuimoError::Ble(e.to_string()))?;
                tracing::debug!("  Characteristic: {}", uuid);
                match uuid {
                    u if u == gatt::LED_MATRIX => led_char = Some(char),
                    u if u == gatt::BATTERY_LEVEL => battery_char = Some(char),
                    u if u == gatt::BUTTON_CLICK => button_char = Some(char),
                    u if u == gatt::ROTATION => rotation_char = Some(char),
                    u if u == gatt::TOUCH_OR_SWIPE => touch_char = Some(char),
                    u if u == gatt::FLY => fly_char = Some(char),
                    _ => {}
                }
            }
        }

        tracing::info!(
            "Characteristics found: led={}, battery={}, button={}, rotation={}, touch={}, fly={}",
            led_char.is_some(),
            battery_char.is_some(),
            button_char.is_some(),
            rotation_char.is_some(),
            touch_char.is_some(),
            fly_char.is_some(),
        );

        if let Some(ref ch) = battery_char {
            if let Ok(data) = ch.read().await {
                if !data.is_empty() {
                    let _ = event_tx.send(NuimoEvent::BatteryLevel(data[0])).await;
                }
            }
        }

        let periph = NuimoPeripheral {
            _session: session,
            device,
            led_char,
            event_tx: event_tx.clone(),
        };

        if let Some(ch) = battery_char {
            spawn_notify_listener(ch, event_tx.clone(), |data| {
                if !data.is_empty() {
                    Some(NuimoEvent::BatteryLevel(data[0]))
                } else {
                    None
                }
            });
        }

        if let Some(ch) = button_char {
            spawn_notify_listener(ch, event_tx.clone(), |data| {
                if !data.is_empty() {
                    match data[0] {
                        1 => Some(NuimoEvent::ButtonDown),
                        0 => Some(NuimoEvent::ButtonUp),
                        _ => None,
                    }
                } else {
                    None
                }
            });
        }

        if let Some(ch) = rotation_char {
            spawn_notify_listener(ch, event_tx.clone(), |data| {
                if data.len() >= 2 {
                    let raw = i16::from_le_bytes([data[0], data[1]]);
                    let delta = raw as f64 / gatt::ROTATION_POINTS_PER_CYCLE;
                    Some(NuimoEvent::Rotate {
                        delta,
                        rotation: 0.0,
                    })
                } else {
                    None
                }
            });
        }

        if let Some(ch) = touch_char {
            spawn_notify_listener(ch, event_tx.clone(), |data| {
                if !data.is_empty() {
                    parse_touch_or_swipe(data[0])
                } else {
                    None
                }
            });
        }

        if let Some(ch) = fly_char {
            spawn_notify_listener(ch, event_tx.clone(), |data| parse_fly(&data));
        }

        let _ = event_tx.send(NuimoEvent::Connected).await;
        spawn_disconnect_monitor(&periph.device, event_tx);

        Ok(periph)
    }

    pub async fn write_display(&self, data: &[u8]) -> Result<(), NuimoError> {
        let ch = self
            .led_char
            .as_ref()
            .ok_or_else(|| NuimoError::CharacteristicNotFound("LED Matrix".into()))?;
        ch.write(data)
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))
    }

    pub async fn disconnect(&self) -> Result<(), NuimoError> {
        self.device
            .disconnect()
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))?;
        let _ = self.event_tx.send(NuimoEvent::Disconnected).await;
        Ok(())
    }

    pub async fn is_connected(&self) -> bool {
        self.device.is_connected().await.unwrap_or(false)
    }

    pub async fn rssi(&self) -> Option<i16> {
        self.device.rssi().await.ok().flatten()
    }
}

fn spawn_notify_listener<F>(char: Characteristic, tx: mpsc::Sender<NuimoEvent>, parser: F)
where
    F: Fn(Vec<u8>) -> Option<NuimoEvent> + Send + 'static,
{
    tokio::spawn(async move {
        let uuid = char.uuid().await.unwrap_or_default();
        tracing::info!("Subscribing to notifications (IO) for {}", uuid);
        match char.notify_io().await {
            Ok(reader) => {
                tracing::info!(
                    "Notification IO established for {} (MTU={})",
                    uuid,
                    reader.mtu()
                );
                loop {
                    match reader.recv().await {
                        Ok(data) => {
                            tracing::debug!("Notification data from {}: {:?}", uuid, data);
                            if let Some(event) = parser(data) {
                                if tx.send(event).await.is_err() {
                                    tracing::warn!("Event channel closed for {}", uuid);
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Notification IO ended for {}: {}", uuid, e);
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Notify IO subscription failed for {}: {}", uuid, e);
            }
        }
    });
}

fn spawn_disconnect_monitor(device: &Device, tx: mpsc::Sender<NuimoEvent>) {
    let device = device.clone();
    tokio::spawn(async move {
        let events = match device.events().await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Failed to monitor device events: {}", e);
                return;
            }
        };
        tokio::pin!(events);
        while let Some(event) = events.next().await {
            if let DeviceEvent::PropertyChanged(DeviceProperty::Connected(false)) = event {
                tracing::info!("Device disconnected (BLE property change)");
                let _ = tx.send(NuimoEvent::Disconnected).await;
                break;
            }
        }
    });
}
