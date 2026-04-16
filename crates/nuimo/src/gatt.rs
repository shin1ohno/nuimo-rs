use uuid::Uuid;

// Service UUIDs
pub const BATTERY_SERVICE: Uuid = Uuid::from_u128(0x0000180f_0000_1000_8000_00805f9b34fb);
pub const LED_SERVICE: Uuid = Uuid::from_u128(0xf29b1523_cb19_40f3_be5c_7241ecb82fd1);
pub const NUIMO_SERVICE: Uuid = Uuid::from_u128(0xf29b1525_cb19_40f3_be5c_7241ecb82fd2);

// Battery characteristic
pub const BATTERY_LEVEL: Uuid = Uuid::from_u128(0x00002a19_0000_1000_8000_00805f9b34fb);

// LED characteristic (write)
pub const LED_MATRIX: Uuid = Uuid::from_u128(0xf29b1524_cb19_40f3_be5c_7241ecb82fd1);

// Nuimo input characteristics (notify)
pub const BUTTON_CLICK: Uuid = Uuid::from_u128(0xf29b1529_cb19_40f3_be5c_7241ecb82fd2);
pub const FLY: Uuid = Uuid::from_u128(0xf29b1526_cb19_40f3_be5c_7241ecb82fd2);
pub const ROTATION: Uuid = Uuid::from_u128(0xf29b1528_cb19_40f3_be5c_7241ecb82fd2);
pub const TOUCH_OR_SWIPE: Uuid = Uuid::from_u128(0xf29b1527_cb19_40f3_be5c_7241ecb82fd2);

// Device constants
pub const DEVICE_NAME: &str = "Nuimo";
pub const ROTATION_POINTS_PER_CYCLE: f64 = 2650.0;
pub const HOVER_PROXIMITY_POINTS: f64 = 250.0;
pub const HOVER_PROXIMITY_MIN_CLAMP: f64 = 2.0;
pub const HOVER_PROXIMITY_MAX_CLAMP: f64 = 1.0;
pub const CONNECT_TIMEOUT_SECS: u64 = 30;

// LED display constants
pub const LED_ROWS: usize = 9;
pub const LED_COLS: usize = 9;
pub const LED_BITMAP_BYTES: usize = 11;
pub const LED_FADE_FLAG: u8 = 0b0001_0000;
