//! BLE backend for nuimo. Linux uses `bluer` (BlueZ D-Bus), macOS uses
//! `btleplug` (CoreBluetooth). Each backend exposes the same surface:
//!   - `DiscoveredNuimo { id, name, adapter_hint }`
//!   - `pub async fn discover() -> Result<(mpsc::Receiver<DiscoveredNuimo>, JoinHandle<()>), NuimoError>`
//!   - `pub struct NuimoPeripheral` with `connect / write_display / disconnect / is_connected / rssi`.
//!
//! `NuimoDevice` and the `nuimo::discover()` public entry point wrap
//! whichever backend was compiled in so downstream code is platform-agnostic.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("nuimo currently supports only Linux (bluer) and macOS (btleplug) — open an issue for other platforms.");
