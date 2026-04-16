use std::collections::HashSet;

use bluer::{Adapter, AdapterEvent, Address, Device};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::error::NuimoError;
use crate::gatt::DEVICE_NAME;

/// A discovered Nuimo device (not yet connected).
#[derive(Debug, Clone)]
pub struct DiscoveredNuimo {
    pub address: Address,
    pub name: String,
    pub adapter: String,
}

/// Scan for Nuimo devices via BLE.
///
/// Returns a receiver that yields `DiscoveredNuimo` for each device found.
/// Duplicates are filtered by address.
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
        if let Err(e) = scan_loop(adapter, tx, &adapter_name).await {
            tracing::warn!("Discovery ended: {}", e);
        }
    });

    Ok((rx, handle))
}

async fn scan_loop(
    adapter: Adapter,
    tx: mpsc::Sender<DiscoveredNuimo>,
    adapter_name: &str,
) -> Result<(), NuimoError> {
    let discover = adapter
        .discover_devices()
        .await
        .map_err(|e| NuimoError::Ble(e.to_string()))?;
    tokio::pin!(discover);

    let mut seen = HashSet::new();

    while let Some(event) = discover.next().await {
        if let AdapterEvent::DeviceAdded(addr) = event {
            if seen.contains(&addr) {
                continue;
            }

            let device = match adapter.device(addr) {
                Ok(d) => d,
                Err(_) => continue,
            };

            if is_nuimo(&device).await {
                seen.insert(addr);
                let name = device
                    .name()
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| DEVICE_NAME.to_string());
                let discovered = DiscoveredNuimo {
                    address: addr,
                    name,
                    adapter: adapter_name.to_string(),
                };
                tracing::info!("Discovered Nuimo: {} ({})", discovered.name, addr);
                if tx.send(discovered).await.is_err() {
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn is_nuimo(device: &Device) -> bool {
    match device.name().await {
        Ok(Some(name)) => name == DEVICE_NAME,
        _ => false,
    }
}
