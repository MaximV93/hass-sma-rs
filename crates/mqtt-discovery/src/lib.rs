//! Home Assistant MQTT discovery publisher.
//!
//! Owns the full catalog of per-inverter sensor configs. Publishing is
//! retained so HA picks them up after a restart even if we're down. Entity
//! naming uses `unique_id + object_id + has_entity_name: true + device` so HA
//! creates clean entity IDs of the form `sensor.sbfspot_<slot>_<metric>`.

pub mod catalog;
pub mod client;
pub mod config;

pub use catalog::{sensor_catalog, SensorConfig};
pub use client::{DiscoveryPublisher, MqttClientConfig, PublisherError};
pub use config::{DeviceKind, InverterIdentity};
