/// Events emitted by a Nuimo device.
#[derive(Debug, Clone, PartialEq)]
pub enum NuimoEvent {
    // Connection
    Connected,
    Disconnected,

    // Button
    ButtonDown,
    ButtonUp,

    // Rotation
    Rotate { delta: f64, rotation: f64 },

    // Swipe (on screen surface)
    SwipeUp,
    SwipeDown,
    SwipeLeft,
    SwipeRight,

    // Touch (tap on screen edge)
    TouchTop,
    TouchBottom,
    TouchLeft,
    TouchRight,

    // Long touch (prolonged hold on screen edge)
    LongTouchLeft,
    LongTouchRight,
    LongTouchTop,
    LongTouchBottom,

    // Fly (in-air gesture)
    FlyLeft,
    FlyRight,

    // Hover (hand proximity, 0.0 = closest, 1.0 = farthest)
    Hover { proximity: f64 },

    // Device state
    BatteryLevel(u8),
    Rssi(i16),
}

/// Raw touch/swipe codes from the BLE characteristic.
pub(crate) fn parse_touch_or_swipe(code: u8) -> Option<NuimoEvent> {
    match code {
        0 => Some(NuimoEvent::SwipeLeft),
        1 => Some(NuimoEvent::SwipeRight),
        2 => Some(NuimoEvent::SwipeUp),
        3 => Some(NuimoEvent::SwipeDown),
        4 => Some(NuimoEvent::TouchLeft),
        5 => Some(NuimoEvent::TouchRight),
        6 => Some(NuimoEvent::TouchTop),
        7 => Some(NuimoEvent::TouchBottom),
        8 => Some(NuimoEvent::LongTouchLeft),
        9 => Some(NuimoEvent::LongTouchRight),
        10 => Some(NuimoEvent::LongTouchTop),
        11 => Some(NuimoEvent::LongTouchBottom),
        _ => None,
    }
}

/// Raw fly codes from the BLE characteristic.
pub(crate) fn parse_fly(data: &[u8]) -> Option<NuimoEvent> {
    if data.is_empty() {
        return None;
    }
    match data[0] {
        0 => Some(NuimoEvent::FlyLeft),
        1 => Some(NuimoEvent::FlyRight),
        4 if data.len() >= 2 => {
            let raw = data[1] as f64;
            let min_clamp = crate::gatt::HOVER_PROXIMITY_MIN_CLAMP;
            let max_clamp = crate::gatt::HOVER_PROXIMITY_MAX_CLAMP;
            let points = crate::gatt::HOVER_PROXIMITY_POINTS;
            let proximity = ((raw - min_clamp) / (points - min_clamp - max_clamp)).clamp(0.0, 1.0);
            Some(NuimoEvent::Hover { proximity })
        }
        _ => None,
    }
}
