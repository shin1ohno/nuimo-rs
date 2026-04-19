mod config;
mod glyphs;
mod mqtt;
mod registry;

use std::time::Duration;

use nuimo::{
    discover, DisplayOptions, DisplayTransition, Glyph, NuimoDevice, NuimoEvent, RotationMode,
};
use rumqttc::AsyncClient;

use crate::registry::GlyphRegistry;

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
    let glyph_registry = GlyphRegistry::new();
    let (mqtt_client, mut reaction_rx) = bridge.start(&device_id, glyph_registry.clone()).await?;
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
                handle_mqtt_reaction(&device, &device_id, &topic, &payload, &glyph_registry).await;
            }
        }
    }
}

async fn handle_nuimo_event(client: &AsyncClient, device_id: &str, event: &NuimoEvent) {
    let (primitive, payload) = match event {
        NuimoEvent::ButtonDown => ("press", serde_json::json!({})),
        NuimoEvent::ButtonUp => ("release", serde_json::json!({})),
        NuimoEvent::Rotate { delta, .. } => ("rotate", serde_json::json!({"delta": delta})),
        NuimoEvent::SwipeUp => ("swipe_up", serde_json::json!({})),
        NuimoEvent::SwipeDown => ("swipe_down", serde_json::json!({})),
        NuimoEvent::SwipeLeft => ("swipe_left", serde_json::json!({})),
        NuimoEvent::SwipeRight => ("swipe_right", serde_json::json!({})),
        NuimoEvent::TouchTop => ("touch_top", serde_json::json!({})),
        NuimoEvent::TouchBottom => ("touch_bottom", serde_json::json!({})),
        NuimoEvent::TouchLeft => ("touch_left", serde_json::json!({})),
        NuimoEvent::TouchRight => ("touch_right", serde_json::json!({})),
        NuimoEvent::LongTouchLeft => ("long_touch_left", serde_json::json!({})),
        NuimoEvent::LongTouchRight => ("long_touch_right", serde_json::json!({})),
        NuimoEvent::LongTouchBottom => ("long_touch_bottom", serde_json::json!({})),
        NuimoEvent::LongTouchTop => ("long_touch_top", serde_json::json!({})),
        NuimoEvent::Hover { proximity } => ("hover", serde_json::json!({"proximity": proximity})),
        NuimoEvent::FlyLeft => ("swipe_left", serde_json::json!({})),
        NuimoEvent::FlyRight => ("swipe_right", serde_json::json!({})),
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

    if let Err(e) = mqtt::publish_input(client, device_id, primitive, &payload).await {
        tracing::warn!("MQTT publish error: {}", e);
    }
}

async fn handle_mqtt_reaction(
    device: &NuimoDevice,
    _device_id: &str,
    _topic: &str,
    payload: &str,
    registry: &GlyphRegistry,
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

    // Map status string → weave glyph name. volume/volumeChange is
    // rendered by the local parametric helper (matches the `volume_bar`
    // builtin on the server).
    let glyph_name = match status {
        "playing" => Some("play"),
        "paused" => Some("pause"),
        "next" => Some("next"),
        "previous" => Some("previous"),
        _ => None,
    };

    let glyph = if status == "volumeChange" {
        glyphs::volume(percentage)
    } else if let Some(name) = glyph_name {
        match registry.get(name).await {
            Some(entry) if !entry.builtin => Glyph::from_str(&entry.pattern),
            Some(_) => {
                tracing::debug!(name, "skipping builtin glyph (rendered locally)");
                return;
            }
            None => {
                tracing::debug!(
                    name,
                    "glyph not in registry yet; feedback suppressed until retained publish arrives"
                );
                return;
            }
        }
    } else {
        glyphs::empty()
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
