use std::time::Duration;

use bluer::gatt::remote::Characteristic;
use bluer::{Adapter, Address, Device, DeviceEvent, DeviceProperty};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::error::NuimoError;
use crate::event::{parse_fly, parse_touch_or_swipe, NuimoEvent};
use crate::gatt;

/// Low-level BLE communication with a Nuimo device.
pub(crate) struct NuimoPeripheral {
    // Keep the session alive so D-Bus notifications continue working.
    _session: bluer::Session,
    device: Device,
    led_char: Option<Characteristic>,
    event_tx: mpsc::Sender<NuimoEvent>,
}

impl NuimoPeripheral {
    pub async fn connect(
        session: bluer::Session,
        adapter: &Adapter,
        address: Address,
        event_tx: mpsc::Sender<NuimoEvent>,
    ) -> Result<Self, NuimoError> {
        let device = adapter
            .device(address)
            .map_err(|e| NuimoError::Ble(e.to_string()))?;

        // Connect with timeout
        tokio::time::timeout(
            Duration::from_secs(gatt::CONNECT_TIMEOUT_SECS),
            device.connect(),
        )
        .await
        .map_err(|_| NuimoError::ConnectionTimeout(gatt::CONNECT_TIMEOUT_SECS))?
        .map_err(|e| NuimoError::Ble(e.to_string()))?;

        tracing::info!("Connected to Nuimo {}", address);

        // Discover services and characteristics
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
            let service_uuid = service.uuid().await.map_err(|e| NuimoError::Ble(e.to_string()))?;
            tracing::debug!("Service: {}", service_uuid);
            let chars = service
                .characteristics()
                .await
                .map_err(|e| NuimoError::Ble(e.to_string()))?;

            for char in chars {
                let uuid = char.uuid().await.map_err(|e| NuimoError::Ble(e.to_string()))?;
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

        // Read battery level
        if let Some(ref ch) = battery_char {
            if let Ok(data) = ch.read().await {
                if !data.is_empty() {
                    let _ = event_tx.send(NuimoEvent::BatteryLevel(data[0])).await;
                }
            }
        }

        // Subscribe to notify characteristics
        let periph = NuimoPeripheral {
            _session: session,
            device,
            led_char,
            event_tx: event_tx.clone(),
        };

        // Spawn notification listeners
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

        // Monitor device connection state for disconnect detection
        spawn_disconnect_monitor(&periph.device, event_tx);

        Ok(periph)
    }

    /// Write a display bitmap to the LED characteristic.
    pub async fn write_display(&self, data: &[u8]) -> Result<(), NuimoError> {
        let ch = self
            .led_char
            .as_ref()
            .ok_or_else(|| NuimoError::CharacteristicNotFound("LED Matrix".into()))?;
        ch.write(data)
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))
    }

    /// Disconnect from the device.
    pub async fn disconnect(&self) -> Result<(), NuimoError> {
        self.device
            .disconnect()
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))?;
        let _ = self.event_tx.send(NuimoEvent::Disconnected).await;
        Ok(())
    }

    /// Check if still connected.
    pub async fn is_connected(&self) -> bool {
        self.device.is_connected().await.unwrap_or(false)
    }

    /// Read RSSI.
    pub async fn rssi(&self) -> Option<i16> {
        self.device.rssi().await.ok().flatten()
    }
}

fn spawn_notify_listener<F>(
    char: Characteristic,
    tx: mpsc::Sender<NuimoEvent>,
    parser: F,
) where
    F: Fn(Vec<u8>) -> Option<NuimoEvent> + Send + 'static,
{
    tokio::spawn(async move {
        let uuid = char.uuid().await.unwrap_or_default();
        tracing::info!("Subscribing to notifications (IO) for {}", uuid);
        match char.notify_io().await {
            Ok(reader) => {
                tracing::info!("Notification IO established for {} (MTU={})", uuid, reader.mtu());
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
