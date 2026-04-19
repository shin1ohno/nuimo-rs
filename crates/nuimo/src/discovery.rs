//! Public BLE discovery entry point. Platform specifics live in
//! `crate::backend` — this module is a thin re-export so downstream code
//! can write `nuimo::discover()` / `nuimo::DiscoveredNuimo` regardless of
//! target OS.

pub use crate::backend::{discover, DiscoveredNuimo};
