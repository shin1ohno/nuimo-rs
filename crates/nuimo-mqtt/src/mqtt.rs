use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use tokio::sync::mpsc;

use crate::config::Config;
use crate::registry::{topic_to_name, GlyphRegistry};

/// MQTT bridge for Nuimo, using weave-compatible topic structure:
///   Publish: device/nuimo/{id}/input/{primitive}
///   Subscribe: device/nuimo/{id}/feedback/{type}
pub struct MqttBridge {
    client: AsyncClient,
    event_loop: EventLoop,
    command_rx: mpsc::Receiver<(String, String)>,
    command_tx: mpsc::Sender<(String, String)>,
}

impl MqttBridge {
    pub fn new(config: &Config) -> Self {
        let url = &config.broker_url;
        let stripped = url
            .strip_prefix("mqtt://")
            .or_else(|| url.strip_prefix("tcp://"))
            .unwrap_or(url);
        let (host, port) = match stripped.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(1883)),
            None => (stripped.to_string(), 1883),
        };

        let mut opts = MqttOptions::new("nuimo-mqtt", &host, port);
        opts.set_keep_alive(std::time::Duration::from_secs(30));
        if let (Some(user), Some(pass)) = (&config.broker_user, &config.broker_password) {
            opts.set_credentials(user, pass);
        }

        let (client, event_loop) = AsyncClient::new(opts, 64);
        let (command_tx, command_rx) = mpsc::channel(64);

        MqttBridge {
            client,
            event_loop,
            command_rx,
            command_tx,
        }
    }

    pub async fn start(
        mut self,
        device_id: &str,
        glyphs: GlyphRegistry,
    ) -> anyhow::Result<(AsyncClient, mpsc::Receiver<(String, String)>)> {
        // Subscribe to feedback from the MQTT routing engine.
        let feedback_topic = format!("device/nuimo/{}/feedback/#", device_id);
        self.client
            .subscribe(&feedback_topic, QoS::AtLeastOnce)
            .await?;
        // Subscribe to the glyph registry so the initial retained publish
        // populates the local cache before any feedback arrives.
        self.client
            .subscribe("system/glyphs/+", QoS::AtLeastOnce)
            .await?;

        let command_tx = self.command_tx.clone();
        let client = self.client.clone();

        tokio::spawn(async move {
            loop {
                match self.event_loop.poll().await {
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(msg))) => {
                        let topic = msg.topic.clone();
                        if let Some(name) = topic_to_name(&topic) {
                            glyphs.apply(name, &msg.payload).await;
                            continue;
                        }
                        if let Ok(payload) = String::from_utf8(msg.payload.to_vec()) {
                            let _ = command_tx.send((topic, payload)).await;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("MQTT error: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Ok((client, self.command_rx))
    }
}

/// Publish an input primitive to device/nuimo/{id}/input/{primitive_name}.
pub async fn publish_input(
    client: &AsyncClient,
    device_id: &str,
    primitive_name: &str,
    payload: &serde_json::Value,
) -> anyhow::Result<()> {
    let topic = format!("device/nuimo/{}/input/{}", device_id, primitive_name);
    client
        .publish(
            &topic,
            QoS::AtMostOnce,
            false,
            serde_json::to_string(payload)?,
        )
        .await?;
    Ok(())
}

/// Publish battery level as device state.
pub async fn publish_battery(
    client: &AsyncClient,
    device_id: &str,
    level: u8,
) -> anyhow::Result<()> {
    let topic = format!("device/nuimo/{}/state/battery", device_id);
    client
        .publish(&topic, QoS::AtLeastOnce, true, level.to_string())
        .await?;
    Ok(())
}

/// Publish RSSI as device state.
pub async fn publish_rssi(client: &AsyncClient, device_id: &str, rssi: i16) -> anyhow::Result<()> {
    let topic = format!("device/nuimo/{}/state/rssi", device_id);
    client
        .publish(&topic, QoS::AtLeastOnce, false, rssi.to_string())
        .await?;
    Ok(())
}

/// Publish device connected event.
pub async fn publish_connected(client: &AsyncClient, device_id: &str) -> anyhow::Result<()> {
    let topic = format!("device/nuimo/{}/state/connected", device_id);
    client
        .publish(&topic, QoS::AtLeastOnce, true, "true")
        .await?;
    Ok(())
}
