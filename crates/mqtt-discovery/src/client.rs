//! MQTT discovery publisher + state topic writer.

use crate::{catalog::sensor_catalog, catalog::SensorConfig, config::InverterIdentity};
use rumqttc::{AsyncClient, MqttOptions, QoS};
use serde_json::{json, Value};
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info};

#[derive(Debug, Error)]
pub enum PublisherError {
    #[error("mqtt client error: {0}")]
    Mqtt(#[from] rumqttc::ClientError),
}

#[derive(Debug, Clone)]
pub struct MqttClientConfig {
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub password: Option<String>,
    pub client_id: String,
    pub keep_alive: Duration,
    pub discovery_prefix: String,
    pub state_prefix: String,
}

impl Default for MqttClientConfig {
    fn default() -> Self {
        Self {
            host: "core-mosquitto".to_string(),
            port: 1883,
            user: None,
            password: None,
            client_id: "hass-sma-rs".to_string(),
            keep_alive: Duration::from_secs(30),
            discovery_prefix: "homeassistant".to_string(),
            state_prefix: "hass-sma".to_string(),
        }
    }
}

/// Publisher owns an MQTT client + event loop and exposes ergonomic
/// methods for announcing sensors and writing state updates.
pub struct DiscoveryPublisher {
    client: AsyncClient,
    cfg: MqttClientConfig,
}

impl DiscoveryPublisher {
    pub async fn connect(cfg: MqttClientConfig) -> Self {
        Self::connect_with_lwt(cfg, None).await
    }

    /// Connect with an optional Last-Will-Testament topic. If provided, the
    /// broker publishes "offline" (retained) to that topic when this client
    /// disconnects uncleanly — so HA can show the inverter as unavailable
    /// rather than showing stale data.
    pub async fn connect_with_lwt(cfg: MqttClientConfig, lwt_topic: Option<String>) -> Self {
        let mut opts = MqttOptions::new(&cfg.client_id, &cfg.host, cfg.port);
        opts.set_keep_alive(cfg.keep_alive);
        if let (Some(u), Some(p)) = (cfg.user.as_ref(), cfg.password.as_ref()) {
            opts.set_credentials(u, p);
        }
        if let Some(topic) = lwt_topic.as_ref() {
            opts.set_last_will(rumqttc::LastWill::new(
                topic,
                "offline",
                QoS::AtLeastOnce,
                true,
            ));
        }
        let (client, mut event_loop) = AsyncClient::new(opts, 64);

        tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(event) => debug!(?event, "mqtt event"),
                    Err(e) => {
                        tracing::warn!(error=%e, "mqtt event loop error; retrying");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
        });

        Self { client, cfg }
    }

    /// Topic for the daemon-level availability signal. HA-discovered
    /// sensors can reference this via `availability_topic` so they flip
    /// to "unavailable" when the daemon goes offline.
    pub fn availability_topic(&self, inv: &InverterIdentity) -> String {
        format!("{}/{}/availability", self.cfg.state_prefix, inv.slot)
    }

    /// Publish "online" to the availability topic. Call right after connect
    /// + after each successful handshake.
    pub async fn publish_online(&self, inv: &InverterIdentity) -> Result<(), PublisherError> {
        let topic = self.availability_topic(inv);
        self.client
            .publish(topic, QoS::AtLeastOnce, true, b"online".to_vec())
            .await?;
        Ok(())
    }

    /// Publish "offline" — manually, on graceful shutdown, or when we
    /// know the inverter went to sleep and repeated reconnects will fail.
    pub async fn publish_offline(&self, inv: &InverterIdentity) -> Result<(), PublisherError> {
        let topic = self.availability_topic(inv);
        self.client
            .publish(topic, QoS::AtLeastOnce, true, b"offline".to_vec())
            .await?;
        Ok(())
    }

    fn discovery_topic(&self, inv: &InverterIdentity, sensor: &SensorConfig) -> String {
        format!(
            "{}/sensor/sbfspot_{}_{}/config",
            self.cfg.discovery_prefix, inv.slot, sensor.key
        )
    }

    fn state_topic(&self, inv: &InverterIdentity, sensor: &SensorConfig) -> String {
        format!("{}/{}/{}", self.cfg.state_prefix, inv.slot, sensor.key)
    }

    /// Publish retained discovery configs for one inverter's entire sensor
    /// catalog. Idempotent on the broker side (retained, overwrites).
    pub async fn announce(&self, inv: &InverterIdentity) -> Result<(), PublisherError> {
        for s in sensor_catalog() {
            let topic = self.discovery_topic(inv, s);
            let state_topic = self.state_topic(inv, s);
            let unique_id = format!("sbfspot_{}_{}_{}", inv.slot, inv.serial, s.key);
            let object_id = format!("sbfspot_{}_{}", inv.slot, s.key);
            let availability_topic = self.availability_topic(inv);
            let mut payload = json!({
                "name": s.name,
                "unique_id": unique_id,
                "object_id": object_id,
                "has_entity_name": true,
                "state_topic": state_topic,
                "availability_topic": availability_topic,
                "payload_available": "online",
                "payload_not_available": "offline",
                "icon": s.icon,
                "device": {
                    "identifiers": [format!("sbfspot_{}", inv.serial)],
                    "name": format!("SBFspot {}", inv.slot),
                    "manufacturer": "SMA (hass-sma-rs)",
                    "model": inv.model,
                    "sw_version": inv.firmware,
                },
            });
            add_optional(&mut payload, "device_class", s.device_class);
            add_optional(&mut payload, "state_class", s.state_class);
            add_optional(&mut payload, "unit_of_measurement", s.unit);
            if s.diagnostic {
                payload["entity_category"] = Value::String("diagnostic".into());
            }

            self.client
                .publish(
                    topic.clone(),
                    QoS::AtLeastOnce,
                    true, // retained
                    serde_json::to_vec(&payload).expect("json serialize"),
                )
                .await?;
            debug!(topic, "discovery published");
        }
        info!(slot = %inv.slot, "announced {} sensors", sensor_catalog().len());
        Ok(())
    }

    /// Write a single sensor value to its state topic.
    pub async fn publish_value<V: ToString>(
        &self,
        inv: &InverterIdentity,
        sensor_key: &str,
        value: V,
    ) -> Result<(), PublisherError> {
        // Look up the sensor in the catalog to build the same topic shape as
        // announce() produced. Avoids cross-file string drift.
        let sensor = sensor_catalog()
            .iter()
            .find(|s| s.key == sensor_key)
            .expect("unknown sensor key");
        let topic = self.state_topic(inv, sensor);
        self.client
            .publish(
                topic,
                QoS::AtMostOnce,
                false,
                value.to_string().into_bytes(),
            )
            .await?;
        Ok(())
    }
}

fn add_optional(obj: &mut Value, field: &str, value: Option<&str>) {
    if let Some(v) = value {
        obj[field] = Value::String(v.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_topic_shape() {
        let cfg = MqttClientConfig::default();
        let pub_ = DiscoveryPublisher {
            client: AsyncClient::new(MqttOptions::new("t", "localhost", 1883), 1).0,
            cfg: cfg.clone(),
        };
        let inv = InverterIdentity {
            slot: "zolder".into(),
            serial: 2_120_121_246,
            model: "SB 3000HF-30".into(),
            firmware: "02.30.06.R".into(),
            kind: crate::DeviceKind::SolarInverter,
        };
        let s = &sensor_catalog()[0]; // ac_power
        assert_eq!(
            pub_.discovery_topic(&inv, s),
            "homeassistant/sensor/sbfspot_zolder_ac_power/config"
        );
        assert_eq!(pub_.state_topic(&inv, s), "hass-sma/zolder/ac_power");
    }
}
