//! macOS BLE backend (btleplug over CoreBluetooth).
//!
//! Mirrors the Linux backend's public surface so `crate::device` and
//! `crate::discovery` are backend-agnostic. On macOS, peripherals are
//! identified by a CoreBluetooth UUID (not a MAC), surfaced as a string
//! via `PeripheralId`'s `Display` impl — downstream code stores it in
//! `DiscoveredNuimo.address` / `NuimoDevice::new(id, ...)` just like on
//! Linux, so no consumer code branches per platform.

use std::sync::Arc;
use std::time::Duration;

use btleplug::api::{
    Central, CentralEvent, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral, PeripheralId};
use futures::StreamExt;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use crate::error::NuimoError;
use crate::event::{parse_fly, parse_touch_or_swipe, NuimoEvent};
use crate::gatt;

#[derive(Debug, Clone)]
pub struct DiscoveredNuimo {
    pub address: String,
    pub name: String,
    pub adapter: String,
}

pub async fn discover(
) -> Result<(mpsc::Receiver<DiscoveredNuimo>, tokio::task::JoinHandle<()>), NuimoError> {
    let manager = Manager::new()
        .await
        .map_err(|e| NuimoError::Ble(e.to_string()))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|e| NuimoError::Ble(e.to_string()))?;
    let central = adapters
        .into_iter()
        .next()
        .ok_or_else(|| NuimoError::Ble("no Bluetooth adapter found".into()))?;
    let adapter_name = "default".to_string();

    central
        .start_scan(ScanFilter::default())
        .await
        .map_err(|e| NuimoError::Ble(e.to_string()))?;

    let (tx, rx) = mpsc::channel::<DiscoveredNuimo>(16);

    let handle = tokio::spawn(async move {
        let mut events = match central.events().await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("CoreBluetooth event stream failed: {}", e);
                return;
            }
        };

        // Catch up on peripherals already known to macOS before we started
        // scanning.
        if let Ok(existing) = central.peripherals().await {
            for p in existing {
                report_if_nuimo(&central, &p, &adapter_name, &tx).await;
            }
        }

        while let Some(event) = events.next().await {
            if let CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) = event {
                if let Ok(p) = central.peripheral(&id).await {
                    report_if_nuimo(&central, &p, &adapter_name, &tx).await;
                }
            }
        }

        tracing::info!("CoreBluetooth scan stream ended");
    });

    Ok((rx, handle))
}

async fn report_if_nuimo(
    _central: &Adapter,
    peripheral: &Peripheral,
    adapter_name: &str,
    tx: &mpsc::Sender<DiscoveredNuimo>,
) {
    let Ok(Some(props)) = peripheral.properties().await else {
        return;
    };
    let Some(name) = props.local_name else {
        return;
    };
    if name != gatt::DEVICE_NAME {
        return;
    }
    let id = peripheral.id().to_string();
    let discovered = DiscoveredNuimo {
        address: id,
        name,
        adapter: adapter_name.to_string(),
    };
    tracing::info!(
        "Discovered Nuimo: {} ({})",
        discovered.name,
        discovered.address,
    );
    let _ = tx.send(discovered).await;
}

pub struct NuimoPeripheral {
    peripheral: Peripheral,
    led_char: Option<Characteristic>,
    event_tx: mpsc::Sender<NuimoEvent>,
    // Hold the adapter reference via Arc so notifications keep flowing even
    // if the caller drops their own reference.
    _adapter: Arc<Adapter>,
}

impl NuimoPeripheral {
    pub async fn connect(
        id: &str,
        _adapter_hint: Option<&str>,
        event_tx: mpsc::Sender<NuimoEvent>,
    ) -> Result<Self, NuimoError> {
        let manager = Manager::new()
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))?;
        let adapters = manager
            .adapters()
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))?;
        let central = Arc::new(
            adapters
                .into_iter()
                .next()
                .ok_or_else(|| NuimoError::Ble("no Bluetooth adapter found".into()))?,
        );

        let peripheral_id = parse_peripheral_id(id)?;
        let peripheral = central
            .peripheral(&peripheral_id)
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))?;

        tokio::time::timeout(
            Duration::from_secs(gatt::CONNECT_TIMEOUT_SECS),
            peripheral.connect(),
        )
        .await
        .map_err(|_| NuimoError::ConnectionTimeout(gatt::CONNECT_TIMEOUT_SECS))?
        .map_err(|e| NuimoError::Ble(e.to_string()))?;

        peripheral
            .discover_services()
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))?;

        let chars = peripheral.characteristics();
        let led_char = find_char(&chars, gatt::LED_MATRIX);
        let battery_char = find_char(&chars, gatt::BATTERY_LEVEL);
        let button_char = find_char(&chars, gatt::BUTTON_CLICK);
        let rotation_char = find_char(&chars, gatt::ROTATION);
        let touch_char = find_char(&chars, gatt::TOUCH_OR_SWIPE);
        let fly_char = find_char(&chars, gatt::FLY);

        tracing::info!(
            "Characteristics found: led={}, battery={}, button={}, rotation={}, touch={}, fly={}",
            led_char.is_some(),
            battery_char.is_some(),
            button_char.is_some(),
            rotation_char.is_some(),
            touch_char.is_some(),
            fly_char.is_some(),
        );

        // Initial battery read.
        if let Some(ref ch) = battery_char {
            if let Ok(data) = peripheral.read(ch).await {
                if !data.is_empty() {
                    let _ = event_tx.send(NuimoEvent::BatteryLevel(data[0])).await;
                }
            }
        }

        // Subscribe to every notify characteristic.
        for ch in [
            &battery_char,
            &button_char,
            &rotation_char,
            &touch_char,
            &fly_char,
        ]
        .iter()
        .filter_map(|c| c.as_ref())
        {
            if let Err(e) = peripheral.subscribe(ch).await {
                tracing::warn!(uuid = %ch.uuid, error = %e, "subscribe failed");
            } else {
                tracing::info!(uuid = %ch.uuid, "subscribed");
            }
        }

        // One notification stream; route by characteristic UUID.
        let notifications = peripheral
            .notifications()
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))?;

        let tx = event_tx.clone();
        let battery_uuid = gatt::BATTERY_LEVEL;
        let button_uuid = gatt::BUTTON_CLICK;
        let rotation_uuid = gatt::ROTATION;
        let touch_uuid = gatt::TOUCH_OR_SWIPE;
        let fly_uuid = gatt::FLY;
        tokio::spawn(async move {
            let mut s = notifications;
            while let Some(n) = s.next().await {
                let data = n.value;
                let parsed = if n.uuid == battery_uuid && !data.is_empty() {
                    Some(NuimoEvent::BatteryLevel(data[0]))
                } else if n.uuid == button_uuid && !data.is_empty() {
                    match data[0] {
                        1 => Some(NuimoEvent::ButtonDown),
                        0 => Some(NuimoEvent::ButtonUp),
                        _ => None,
                    }
                } else if n.uuid == rotation_uuid && data.len() >= 2 {
                    let raw = i16::from_le_bytes([data[0], data[1]]);
                    let delta = raw as f64 / gatt::ROTATION_POINTS_PER_CYCLE;
                    Some(NuimoEvent::Rotate {
                        delta,
                        rotation: 0.0,
                    })
                } else if n.uuid == touch_uuid && !data.is_empty() {
                    parse_touch_or_swipe(data[0])
                } else if n.uuid == fly_uuid {
                    parse_fly(&data)
                } else {
                    None
                };
                if let Some(ev) = parsed {
                    if tx.send(ev).await.is_err() {
                        break;
                    }
                }
            }
            tracing::info!("Notification stream ended");
        });

        // Disconnect monitor: poll is_connected periodically because
        // btleplug's CentralEvent stream is shared with discover() and we
        // want a per-peripheral signal.
        spawn_disconnect_monitor(peripheral.clone(), event_tx.clone());

        let _ = event_tx.send(NuimoEvent::Connected).await;

        Ok(Self {
            peripheral,
            led_char,
            event_tx,
            _adapter: central,
        })
    }

    pub async fn write_display(&self, data: &[u8]) -> Result<(), NuimoError> {
        let ch = self
            .led_char
            .as_ref()
            .ok_or_else(|| NuimoError::CharacteristicNotFound("LED Matrix".into()))?;
        self.peripheral
            .write(ch, data, WriteType::WithResponse)
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))
    }

    pub async fn disconnect(&self) -> Result<(), NuimoError> {
        self.peripheral
            .disconnect()
            .await
            .map_err(|e| NuimoError::Ble(e.to_string()))?;
        let _ = self.event_tx.send(NuimoEvent::Disconnected).await;
        Ok(())
    }

    pub async fn is_connected(&self) -> bool {
        self.peripheral.is_connected().await.unwrap_or(false)
    }

    pub async fn rssi(&self) -> Option<i16> {
        match self.peripheral.properties().await.ok().flatten() {
            Some(props) => props.rssi,
            None => None,
        }
    }
}

fn find_char(
    chars: &std::collections::BTreeSet<Characteristic>,
    uuid: Uuid,
) -> Option<Characteristic> {
    chars.iter().find(|c| c.uuid == uuid).cloned()
}

fn parse_peripheral_id(id: &str) -> Result<PeripheralId, NuimoError> {
    // btleplug on macOS wraps CoreBluetooth's NSUUID. The stable
    // representation we pass around is its Display string. Constructing a
    // PeripheralId directly from a string is not a public API — but the
    // user holds onto the id they got from `discover()`, which already
    // carries the right value. As a safety check, verify it parses as a
    // UUID; fail fast otherwise so downstream errors are understandable.
    Uuid::parse_str(id)
        .map_err(|_| NuimoError::Ble(format!("invalid peripheral id (expected UUID): {id}")))?;
    // Use the Uuid parse as the id round-trip. btleplug exposes
    // `PeripheralId::from(Uuid)` on macOS builds.
    Ok(PeripheralId::from(Uuid::parse_str(id).unwrap()))
}

fn spawn_disconnect_monitor(peripheral: Peripheral, tx: mpsc::Sender<NuimoEvent>) {
    let periph = Arc::new(Mutex::new(peripheral));
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let p = periph.lock().await;
            let connected = p.is_connected().await.unwrap_or(false);
            if !connected {
                tracing::info!("Device disconnected (poll)");
                let _ = tx.send(NuimoEvent::Disconnected).await;
                break;
            }
        }
    });
}
