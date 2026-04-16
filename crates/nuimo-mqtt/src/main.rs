mod config;
mod glyphs;
mod mqtt;

use std::time::Duration;

use nuimo::{
    discover, DisplayOptions, DisplayTransition, Glyph, NuimoDevice, NuimoEvent, RotationMode,
};
use rumqttc::AsyncClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = config::Config::from_env();

    // Discover Nuimo
    tracing::info!("Scanning for Nuimo devices...");
    let (mut rx, _handle) = discover().await?;

    let discovered = rx
        .recv()
        .await
        .ok_or_else(|| anyhow::anyhow!("No Nuimo device found"))?;

    tracing::info!("Found: {} ({})", discovered.name, discovered.address);

    // Create device and connect
    let device = NuimoDevice::new(discovered.address, &discovered.adapter);
    device.connect().await?;
    device.set_rotation_mode(RotationMode::Continuous).await;

    let device_id = device.id();
    tracing::info!("Connected to Nuimo {}", device_id);

    // Show link glyph on connection
    device
        .display_glyph(
            &glyphs::link(),
            &DisplayOptions {
                brightness: 1.0,
                timeout_ms: 3000,
                transition: DisplayTransition::CrossFade,
            },
        )
        .await?;

    // Connect MQTT
    let bridge = mqtt::MqttBridge::new(&config);
    let (mqtt_client, mut reaction_rx) = bridge.start(&device_id).await?;
    mqtt::publish_connected(&mqtt_client, &device_id).await?;
    tracing::info!("MQTT connected");

    // Subscribe to Nuimo events
    let mut events = device.events();

    // Heartbeat state
    let heartbeat_client = mqtt_client.clone();
    let heartbeat_device_id = device_id.clone();
    let heartbeat_active = std::sync::Arc::new(tokio::sync::Mutex::new(true));
    let heartbeat_flag = heartbeat_active.clone();

    // Heartbeat task: publish RSSI + battery every 4s
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(4));
        let mut ticks = 0u32;
        loop {
            interval.tick().await;
            if !*heartbeat_flag.lock().await {
                continue;
            }
            ticks += 1;
            if ticks > 300 {
                // Stop after 20 minutes (300 * 4s)
                *heartbeat_flag.lock().await = false;
                continue;
            }
            // Battery and RSSI would be read from device if connected
            // For now, publish placeholder
            let _ =
                mqtt::publish_rssi(&heartbeat_client, &heartbeat_device_id, -50).await;
        }
    });

    // Main event loop
    loop {
        tokio::select! {
            // Nuimo → MQTT
            Ok(event) = events.recv() => {
                *heartbeat_active.lock().await = true;
                handle_nuimo_event(&mqtt_client, &device_id, &event).await;
            }

            // MQTT → Nuimo (reactions)
            Some((topic, payload)) = reaction_rx.recv() => {
                *heartbeat_active.lock().await = true;
                handle_mqtt_reaction(&device, &device_id, &topic, &payload).await;
            }
        }
    }
}

async fn handle_nuimo_event(client: &AsyncClient, device_id: &str, event: &NuimoEvent) {
    let (subject, parameter) = match event {
        NuimoEvent::ButtonDown => ("selectDown", serde_json::json!(1)),
        NuimoEvent::ButtonUp => ("selectUp", serde_json::json!(0)),
        NuimoEvent::Rotate { delta, .. } => ("rotate", serde_json::json!(delta)),
        NuimoEvent::SwipeUp => ("swipeUp", serde_json::json!(null)),
        NuimoEvent::SwipeDown => ("swipeDown", serde_json::json!(null)),
        NuimoEvent::SwipeLeft => ("swipeLeft", serde_json::json!(null)),
        NuimoEvent::SwipeRight => ("swipeRight", serde_json::json!(null)),
        NuimoEvent::TouchTop => ("touchTop", serde_json::json!(null)),
        NuimoEvent::TouchBottom => ("touchBottom", serde_json::json!(null)),
        NuimoEvent::TouchLeft => ("touchLeft", serde_json::json!(null)),
        NuimoEvent::TouchRight => ("touchRight", serde_json::json!(null)),
        NuimoEvent::LongTouchLeft => ("longTouchLeft", serde_json::json!(null)),
        NuimoEvent::LongTouchRight => ("longTouchRight", serde_json::json!(null)),
        NuimoEvent::LongTouchBottom => ("longTouchBottom", serde_json::json!(null)),
        NuimoEvent::LongTouchTop => ("longTouchTop", serde_json::json!(null)),
        NuimoEvent::Hover { proximity } => ("hover", serde_json::json!(proximity)),
        NuimoEvent::FlyLeft => ("swipeLeft", serde_json::json!({"hoverSwipe": true})),
        NuimoEvent::FlyRight => ("swipeRight", serde_json::json!({"hoverSwipe": true})),
        NuimoEvent::BatteryLevel(level) => {
            let _ = mqtt::publish_battery(client, device_id, *level).await;
            return;
        }
        NuimoEvent::Rssi(rssi) => {
            let _ = mqtt::publish_rssi(client, device_id, *rssi).await;
            return;
        }
        NuimoEvent::Connected | NuimoEvent::Disconnected => return,
    };

    if let Err(e) = mqtt::publish_operation(client, device_id, subject, &parameter).await {
        tracing::warn!("MQTT publish error: {}", e);
    }
}

async fn handle_mqtt_reaction(
    device: &NuimoDevice,
    _device_id: &str,
    _topic: &str,
    payload: &str,
) {
    let body: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Invalid reaction JSON: {}", e);
            return;
        }
    };

    let status = body["status"].as_str().unwrap_or("");
    let percentage = body["percentage"]
        .as_str()
        .and_then(|s| s.parse::<u8>().ok())
        .or_else(|| body["percentage"].as_u64().map(|v| v as u8))
        .unwrap_or(0);

    let glyph = match status {
        "playing" => glyphs::play(),
        "paused" => glyphs::pause(),
        "next" => glyphs::next(),
        "previous" => glyphs::previous(),
        "volumeChange" => glyphs::volume(percentage),
        _ => glyphs::empty(),
    };

    let transition = if status == "volumeChange" {
        DisplayTransition::Immediate
    } else {
        DisplayTransition::CrossFade
    };

    if let Err(e) = device
        .display_glyph(
            &glyph,
            &DisplayOptions {
                brightness: 1.0,
                timeout_ms: 1000,
                transition,
            },
        )
        .await
    {
        tracing::warn!("Display error: {}", e);
    }
}
