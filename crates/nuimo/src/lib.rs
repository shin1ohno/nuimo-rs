pub mod device;
pub mod discovery;
pub mod error;
pub mod event;
pub mod gatt;
pub mod glyph;
pub(crate) mod peripheral;

pub use device::{NuimoDevice, RotationMode};
pub use discovery::{discover, DiscoveredNuimo};
pub use error::NuimoError;
pub use event::NuimoEvent;
pub use glyph::{DisplayOptions, DisplayTransition, Glyph};
