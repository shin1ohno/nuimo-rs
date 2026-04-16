#[derive(Debug, thiserror::Error)]
pub enum NuimoError {
    #[error("BLE error: {0}")]
    Ble(String),
    #[error("connection timeout ({0}s)")]
    ConnectionTimeout(u64),
    #[error("not connected")]
    NotConnected,
    #[error("device not available")]
    NotAvailable,
    #[error("characteristic not found: {0}")]
    CharacteristicNotFound(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
