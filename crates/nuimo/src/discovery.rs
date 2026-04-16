use std::time::Duration;

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
        // Keep _session alive so the adapter's D-Bus connection persists
        let _session = session;

        // Periodically check BlueZ-cached devices (handles devices that were
        // already known before scanning started, or that reappear after failures)
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

        // Run the live BLE scan stream with auto-restart
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
    // Check devices already known to BlueZ (from previous scans / pairing)
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
        .unwrap_or_else(|| DEVICE_NAME.to_string());
    let discovered = DiscoveredNuimo {
        address: addr,
        name,
        adapter: adapter_name.to_string(),
    };
    tracing::info!("Discovered Nuimo: {} ({})", discovered.name, addr);
    let _ = tx.send(discovered).await;
}

async fn is_nuimo(device: &Device) -> bool {
    match device.name().await {
        Ok(Some(name)) => name == DEVICE_NAME,
        _ => false,
    }
}
