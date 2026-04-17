//! Typed daemon configuration loaded from YAML.
//!
//! Example:
//!
//! ```yaml
//! mqtt:
//!   host: core-mosquitto
//!   port: 1883
//!   user: sbfspot
//!   password: !secret mqtt_pass
//!
//! inverters:
//!   - slot: zolder
//!     bt_address: "00:80:25:21:32:35"
//!     password: "0000"
//!     poll_interval: 5s
//!   - slot: garage
//!     bt_address: "00:80:25:21:32:36"
//!     password: "0000"
//!     poll_interval: 5s
//!
//! local_bt_address: "04:42:1A:5A:37:74"
//! ```

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DaemonConfig {
    pub mqtt: MqttCfg,
    pub inverters: Vec<InverterCfg>,
    /// Optional local adapter MAC; when absent, kernel picks automatically.
    #[serde(default)]
    pub local_bt_address: Option<String>,
    /// RFCOMM recv timeout per frame.
    #[serde(with = "humantime_serde", default = "default_rfcomm_timeout")]
    pub rfcomm_timeout: Duration,
    /// Prometheus /metrics endpoint bind address. Default "0.0.0.0:9090".
    #[serde(default = "default_metrics_addr")]
    pub metrics_addr: String,
}

fn default_metrics_addr() -> String {
    "0.0.0.0:9090".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MqttCfg {
    pub host: String,
    #[serde(default = "default_mqtt_port")]
    pub port: u16,
    pub user: Option<String>,
    pub password: Option<String>,
    #[serde(default = "default_client_id")]
    pub client_id: String,
    #[serde(default = "default_discovery_prefix")]
    pub discovery_prefix: String,
    #[serde(default = "default_state_prefix")]
    pub state_prefix: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InverterCfg {
    pub slot: String,
    pub bt_address: String,
    pub password: String,
    #[serde(with = "humantime_serde", default = "default_poll")]
    pub poll_interval: Duration,
    /// Optional fixed model string (shows up in HA device card). Overridden
    /// at runtime once we read real type-label from the inverter.
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_firmware")]
    pub firmware: String,
    /// Multi-inverter support on this piconet.
    #[serde(default)]
    pub mis_enabled: bool,
}

fn default_mqtt_port() -> u16 {
    1883
}
fn default_client_id() -> String {
    "hass-sma-rs".into()
}
fn default_discovery_prefix() -> String {
    "homeassistant".into()
}
fn default_state_prefix() -> String {
    "hass-sma".into()
}
fn default_rfcomm_timeout() -> Duration {
    Duration::from_secs(15)
}
fn default_poll() -> Duration {
    Duration::from_secs(5)
}
fn default_model() -> String {
    "unknown".into()
}
fn default_firmware() -> String {
    "unknown".into()
}

impl DaemonConfig {
    pub fn from_yaml(src: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(src)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAML: &str = r#"
mqtt:
  host: core-mosquitto
  user: sbfspot
  password: hunter2

inverters:
  - slot: zolder
    bt_address: "00:80:25:21:32:35"
    password: "0000"
    poll_interval: 5s
    model: "SB 3000HF-30"
    firmware: "02.30.06.R"

local_bt_address: "04:42:1A:5A:37:74"
"#;

    #[test]
    fn parses_minimal_yaml() {
        let cfg = DaemonConfig::from_yaml(YAML).unwrap();
        assert_eq!(cfg.mqtt.host, "core-mosquitto");
        assert_eq!(cfg.mqtt.port, 1883);
        assert_eq!(cfg.inverters.len(), 1);
        assert_eq!(cfg.inverters[0].slot, "zolder");
        assert_eq!(cfg.inverters[0].poll_interval, Duration::from_secs(5));
        assert_eq!(cfg.local_bt_address.as_deref(), Some("04:42:1A:5A:37:74"));
    }

    #[test]
    fn defaults_when_optional_absent() {
        let minimal = r#"
mqtt:
  host: core-mosquitto
inverters: []
"#;
        let cfg = DaemonConfig::from_yaml(minimal).unwrap();
        assert_eq!(cfg.mqtt.client_id, "hass-sma-rs");
        assert_eq!(cfg.mqtt.discovery_prefix, "homeassistant");
        assert_eq!(cfg.rfcomm_timeout, Duration::from_secs(15));
        assert!(cfg.local_bt_address.is_none());
    }
}
