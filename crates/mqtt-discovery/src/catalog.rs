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
/// Ordering note: measurement sensors first, then energy counters, then
/// DC-side, then diagnostic sensors (grouped at the bottom of HA's device
/// card via `entity_category: diagnostic`).
pub fn sensor_catalog() -> &'static [SensorConfig] {
    &[
        // --- AC side (primary measurements) ---
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
            name: "AC Power L1",
            key: "ac_power_l1",
            device_class: Some("power"),
            state_class: Some("measurement"),
            unit: Some("W"),
            icon: "mdi:flash",
            diagnostic: false,
        },
        SensorConfig {
            name: "AC Power L2",
            key: "ac_power_l2",
            device_class: Some("power"),
            state_class: Some("measurement"),
            unit: Some("W"),
            icon: "mdi:flash",
            diagnostic: false,
        },
        SensorConfig {
            name: "AC Power L3",
            key: "ac_power_l3",
            device_class: Some("power"),
            state_class: Some("measurement"),
            unit: Some("W"),
            icon: "mdi:flash",
            diagnostic: false,
        },
        SensorConfig {
            name: "AC Voltage L1",
            key: "ac_voltage_l1",
            device_class: Some("voltage"),
            state_class: Some("measurement"),
            unit: Some("V"),
            icon: "mdi:flash-triangle-outline",
            diagnostic: false,
        },
        SensorConfig {
            name: "AC Voltage L2",
            key: "ac_voltage_l2",
            device_class: Some("voltage"),
            state_class: Some("measurement"),
            unit: Some("V"),
            icon: "mdi:flash-triangle-outline",
            diagnostic: false,
        },
        SensorConfig {
            name: "AC Voltage L3",
            key: "ac_voltage_l3",
            device_class: Some("voltage"),
            state_class: Some("measurement"),
            unit: Some("V"),
            icon: "mdi:flash-triangle-outline",
            diagnostic: false,
        },
        SensorConfig {
            name: "AC Current L1",
            key: "ac_current_l1",
            device_class: Some("current"),
            state_class: Some("measurement"),
            unit: Some("A"),
            icon: "mdi:current-ac",
            diagnostic: false,
        },
        SensorConfig {
            name: "AC Current L2",
            key: "ac_current_l2",
            device_class: Some("current"),
            state_class: Some("measurement"),
            unit: Some("A"),
            icon: "mdi:current-ac",
            diagnostic: false,
        },
        SensorConfig {
            name: "AC Current L3",
            key: "ac_current_l3",
            device_class: Some("current"),
            state_class: Some("measurement"),
            unit: Some("A"),
            icon: "mdi:current-ac",
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
        // --- DC side (per string) ---
        SensorConfig {
            name: "DC Power String 1",
            key: "dc_power_s1",
            device_class: Some("power"),
            state_class: Some("measurement"),
            unit: Some("W"),
            icon: "mdi:solar-panel",
            diagnostic: false,
        },
        SensorConfig {
            name: "DC Power String 2",
            key: "dc_power_s2",
            device_class: Some("power"),
            state_class: Some("measurement"),
            unit: Some("W"),
            icon: "mdi:solar-panel",
            diagnostic: false,
        },
        SensorConfig {
            name: "DC Voltage String 1",
            key: "dc_voltage_s1",
            device_class: Some("voltage"),
            state_class: Some("measurement"),
            unit: Some("V"),
            icon: "mdi:flash-triangle-outline",
            diagnostic: false,
        },
        SensorConfig {
            name: "DC Voltage String 2",
            key: "dc_voltage_s2",
            device_class: Some("voltage"),
            state_class: Some("measurement"),
            unit: Some("V"),
            icon: "mdi:flash-triangle-outline",
            diagnostic: false,
        },
        SensorConfig {
            name: "DC Current String 1",
            key: "dc_current_s1",
            device_class: Some("current"),
            state_class: Some("measurement"),
            unit: Some("A"),
            icon: "mdi:current-dc",
            diagnostic: false,
        },
        SensorConfig {
            name: "DC Current String 2",
            key: "dc_current_s2",
            device_class: Some("current"),
            state_class: Some("measurement"),
            unit: Some("A"),
            icon: "mdi:current-dc",
            diagnostic: false,
        },
        // --- Energy counters ---
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
            name: "Operation Time",
            key: "operation_time",
            device_class: Some("duration"),
            state_class: Some("total_increasing"),
            unit: Some("h"),
            icon: "mdi:timer-sand",
            diagnostic: true,
        },
        SensorConfig {
            name: "Feed-in Time",
            key: "feed_in_time",
            device_class: Some("duration"),
            state_class: Some("total_increasing"),
            unit: Some("h"),
            icon: "mdi:transmission-tower",
            diagnostic: true,
        },
        // --- Temperature ---
        SensorConfig {
            name: "Inverter Temperature",
            key: "temperature",
            device_class: Some("temperature"),
            state_class: Some("measurement"),
            unit: Some("°C"),
            icon: "mdi:thermometer",
            diagnostic: false,
        },
        // --- Diagnostic / identity ---
        SensorConfig {
            name: "Inverter Status",
            key: "inverter_state",
            device_class: None,
            state_class: None,
            unit: None,
            icon: "mdi:heart-pulse",
            diagnostic: true,
        },
        SensorConfig {
            name: "Poll Status",
            key: "poll_status",
            device_class: None,
            state_class: None,
            unit: None,
            icon: "mdi:check-network-outline",
            diagnostic: true,
        },
        SensorConfig {
            name: "Grid Relay",
            key: "grid_relay",
            device_class: None,
            state_class: None,
            unit: None,
            icon: "mdi:electric-switch",
            diagnostic: true,
        },
        SensorConfig {
            name: "Inverter Model",
            key: "inverter_model",
            device_class: None,
            state_class: None,
            unit: None,
            icon: "mdi:solar-panel-large",
            diagnostic: true,
        },
        SensorConfig {
            name: "Firmware Version",
            key: "firmware_version",
            device_class: None,
            state_class: None,
            unit: None,
            icon: "mdi:chip",
            diagnostic: true,
        },
        SensorConfig {
            name: "Session Uptime",
            key: "session_uptime",
            device_class: Some("duration"),
            state_class: Some("measurement"),
            unit: Some("s"),
            icon: "mdi:timer-outline",
            diagnostic: true,
        },
        SensorConfig {
            name: "Last Poll",
            key: "last_poll",
            device_class: Some("timestamp"),
            state_class: None,
            unit: None,
            icon: "mdi:clock-check",
            diagnostic: true,
        },
        SensorConfig {
            name: "Last Event",
            key: "last_event",
            device_class: None,
            state_class: None,
            unit: None,
            icon: "mdi:alert-circle-outline",
            diagnostic: true,
        },
        SensorConfig {
            name: "Last Event At",
            key: "last_event_at",
            device_class: Some("timestamp"),
            state_class: None,
            unit: None,
            icon: "mdi:clock-alert-outline",
            diagnostic: true,
        },
        SensorConfig {
            name: "Event Count 24h",
            key: "last_event_count_24h",
            device_class: None,
            state_class: Some("measurement"),
            unit: None,
            icon: "mdi:counter",
            diagnostic: true,
        },
        // 4 extended LRIs shipped 0.1.50. Experimental until live-validated.
        SensorConfig {
            name: "Power Factor",
            key: "cos_phi",
            device_class: Some("power_factor"),
            state_class: Some("measurement"),
            unit: None,
            icon: "mdi:angle-acute",
            diagnostic: false,
        },
        SensorConfig {
            name: "Max Feed-In Power",
            key: "max_feedin_w",
            device_class: Some("power"),
            state_class: Some("measurement"),
            unit: Some("W"),
            icon: "mdi:transmission-tower-export",
            diagnostic: true,
        },
        SensorConfig {
            name: "Nominal AC Power",
            key: "nominal_ac_power_w",
            device_class: Some("power"),
            state_class: Some("measurement"),
            unit: Some("W"),
            icon: "mdi:tag",
            diagnostic: true,
        },
        // `active_power_limit_w` sensor removed 0.1.51 — HF-30 firmware
        // doesn't expose the LRI. Re-add for newer inverters.
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
