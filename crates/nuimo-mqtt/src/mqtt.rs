use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use tokio::sync::mpsc;

use crate::config::Config;

pub struct MqttBridge {
    client: AsyncClient,
    event_loop: EventLoop,
    command_rx: mpsc::Receiver<(String, String)>,
    command_tx: mpsc::Sender<(String, String)>,
}

impl MqttBridge {
    pub fn new(config: &Config) -> Self {
        let url = &config.broker_url;
        // Parse host:port from mqtt://host:port
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
    ) -> anyhow::Result<(AsyncClient, mpsc::Receiver<(String, String)>)> {
        let reaction_topic = format!("nuimo/{}/reaction", device_id);
        self.client
            .subscribe(&reaction_topic, QoS::AtLeastOnce)
            .await?;

        let command_tx = self.command_tx.clone();
        let client = self.client.clone();

        tokio::spawn(async move {
            loop {
                match self.event_loop.poll().await {
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(msg))) => {
                        let topic = msg.topic.clone();
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

pub async fn publish_operation(
    client: &AsyncClient,
    device_id: &str,
    subject: &str,
    parameter: &serde_json::Value,
) -> anyhow::Result<()> {
    let topic = format!("nuimo/{}/operation", device_id);
    let payload = serde_json::json!({
        "subject": subject,
        "parameter": parameter,
    });
    client
        .publish(&topic, QoS::AtLeastOnce, false, serde_json::to_string(&payload)?)
        .await?;
    Ok(())
}

pub async fn publish_rssi(
    client: &AsyncClient,
    device_id: &str,
    rssi: i16,
) -> anyhow::Result<()> {
    let topic = format!("nuimo/{}/rssi", device_id);
    client
        .publish(&topic, QoS::AtLeastOnce, false, rssi.to_string())
        .await?;
    Ok(())
}

pub async fn publish_battery(
    client: &AsyncClient,
    device_id: &str,
    level: u8,
) -> anyhow::Result<()> {
    let topic = format!("nuimo/{}/batteryLevel", device_id);
    client
        .publish(&topic, QoS::AtLeastOnce, false, level.to_string())
        .await?;
    Ok(())
}

pub async fn publish_connected(
    client: &AsyncClient,
    device_id: &str,
) -> anyhow::Result<()> {
    client
        .publish("nuimo/connected", QoS::AtLeastOnce, false, device_id)
        .await?;
    Ok(())
}
