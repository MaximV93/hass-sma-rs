//! Sensor catalog — the list of per-inverter sensors we own.

use serde::{Deserialize, Serialize};

/// One sensor's HA discovery attributes (subset we actually use).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorConfig {
    /// Display name as it appears on the HA card. HA prepends the device name.
    pub name: &'static str,
    /// Short machine key for entity_id + unique_id construction.
    pub key: &'static str,
    /// HA device_class (`power`, `energy`, `temperature`, ...). None → text.
    pub device_class: Option<&'static str>,
    /// HA state_class (`measurement`, `total_increasing`, `total`). None → stateless text.
    pub state_class: Option<&'static str>,
    /// SI / engineering unit displayed with the value.
    pub unit: Option<&'static str>,
    /// Material Design Icon identifier.
    pub icon: &'static str,
    /// Mark as `diagnostic` category for health-related signals.
    pub diagnostic: bool,
}

/// The full sensor catalog we publish per inverter.
///
/// Keep this focused on the values we actually parse. Extending the set is
/// additive: add an entry here, wire parsing in `inverter-client::values`,
/// and the next publisher tick picks it up.
pub fn sensor_catalog() -> &'static [SensorConfig] {
    &[
        SensorConfig {
            name: "AC Power",
            key: "ac_power",
            device_class: Some("power"),
            state_class: Some("measurement"),
            unit: Some("W"),
            icon: "mdi:flash",
            diagnostic: false,
        },
        SensorConfig {
            name: "Energy Today",
            key: "energy_today",
            device_class: Some("energy"),
            state_class: Some("total_increasing"),
            unit: Some("kWh"),
            icon: "mdi:solar-power-variant",
            diagnostic: false,
        },
        SensorConfig {
            name: "Energy Lifetime",
            key: "energy_lifetime",
            device_class: Some("energy"),
            state_class: Some("total_increasing"),
            unit: Some("kWh"),
            icon: "mdi:counter",
            diagnostic: false,
        },
        SensorConfig {
            name: "Inverter Temperature",
            key: "temperature",
            device_class: Some("temperature"),
            state_class: Some("measurement"),
            unit: Some("°C"),
            icon: "mdi:thermometer",
            diagnostic: false,
        },
        SensorConfig {
            name: "Grid Frequency",
            key: "grid_frequency",
            device_class: Some("frequency"),
            state_class: Some("measurement"),
            unit: Some("Hz"),
            icon: "mdi:sine-wave",
            diagnostic: false,
        },
        SensorConfig {
            name: "Inverter Status",
            key: "status",
            device_class: None,
            state_class: None,
            unit: None,
            icon: "mdi:heart-pulse",
            diagnostic: true,
        },
        SensorConfig {
            name: "BT Signal",
            key: "bt_signal",
            device_class: None,
            state_class: Some("measurement"),
            unit: Some("%"),
            icon: "mdi:bluetooth-audio",
            diagnostic: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_keys_are_unique() {
        let keys: Vec<_> = sensor_catalog().iter().map(|s| s.key).collect();
        let mut dedup = keys.clone();
        dedup.sort_unstable();
        dedup.dedup();
        assert_eq!(dedup.len(), keys.len(), "duplicate sensor key in catalog");
    }

    #[test]
    fn all_entries_have_non_empty_name() {
        for s in sensor_catalog() {
            assert!(!s.name.is_empty());
            assert!(!s.key.is_empty());
            assert!(!s.icon.is_empty());
        }
    }
}
