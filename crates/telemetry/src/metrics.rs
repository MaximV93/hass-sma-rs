//! Prometheus metrics registry.

use prometheus_client::{
    encoding::text::encode,
    metrics::{counter::Counter, family::Family, gauge::Gauge},
    registry::Registry,
};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Session-level labels (one series per `inverters:` YAML entry — i.e.
/// one per RFCOMM session). Used for connection lifecycle metrics
/// that don't have a per-device split: reconnects, handshake errors,
/// session-wide aliveness.
#[derive(Clone, Debug, Eq, Hash, PartialEq, prometheus_client::encoding::EncodeLabelSet)]
pub struct InverterLabels {
    pub slot: String,
}

/// Per-device labels. In legacy single-device mode `device == slot` so
/// existing Grafana queries using `{slot="zolder"}` still work after
/// joining on either label. In MIS mode `slot` = the repeater's
/// config slot, `device` = the actual inverter slot (zolder / garage).
#[derive(Clone, Debug, Eq, Hash, PartialEq, prometheus_client::encoding::EncodeLabelSet)]
pub struct DeviceLabels {
    pub slot: String,
    pub device: String,
}

impl DeviceLabels {
    /// Helper: construct for a device whose slot == its RFCOMM
    /// session's slot (the legacy single-device case).
    pub fn same(s: impl Into<String>) -> Self {
        let s = s.into();
        Self {
            slot: s.clone(),
            device: s,
        }
    }
}

/// The daemon's canonical metric set. Cheap to clone (Arc internally).
#[derive(Clone)]
pub struct MetricsRegistry {
    pub registry: Arc<Mutex<Registry>>,
    // --- Session lifecycle ---
    pub polls_total: Family<InverterLabels, Counter>,
    pub poll_errors_total: Family<InverterLabels, Counter>,
    pub bt_reconnects_total: Family<InverterLabels, Counter>,
    pub handshake_errors_total: Family<InverterLabels, Counter>,
    pub inverter_awake: Family<InverterLabels, Gauge<i64>>,
    pub last_successful_poll_unix: Family<InverterLabels, Gauge<i64>>,
    // --- AC/DC live values (per-device for MIS-accurate observability) ---
    pub ac_power_watts: Family<DeviceLabels, Gauge<f64, std::sync::atomic::AtomicU64>>,
    pub ac_voltage_l1: Family<DeviceLabels, Gauge<f64, std::sync::atomic::AtomicU64>>,
    pub ac_current_l1: Family<DeviceLabels, Gauge<f64, std::sync::atomic::AtomicU64>>,
    pub grid_frequency_hz: Family<DeviceLabels, Gauge<f64, std::sync::atomic::AtomicU64>>,
    pub dc_power_s1_watts: Family<DeviceLabels, Gauge<f64, std::sync::atomic::AtomicU64>>,
    pub dc_power_s2_watts: Family<DeviceLabels, Gauge<f64, std::sync::atomic::AtomicU64>>,
    pub inverter_temperature_c: Family<DeviceLabels, Gauge<f64, std::sync::atomic::AtomicU64>>,
    // --- Energy counters (per-device, monotonically increasing) ---
    pub energy_today_wh: Family<DeviceLabels, Gauge<f64, std::sync::atomic::AtomicU64>>,
    pub energy_lifetime_wh: Family<DeviceLabels, Gauge<f64, std::sync::atomic::AtomicU64>>,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        let mut registry = Registry::default();

        let polls_total = Family::<InverterLabels, Counter>::default();
        registry.register(
            "sma_polls_total",
            "Total number of poll cycles completed",
            polls_total.clone(),
        );

        let poll_errors_total = Family::<InverterLabels, Counter>::default();
        registry.register(
            "sma_poll_errors_total",
            "Total number of polls that returned an error",
            poll_errors_total.clone(),
        );

        let bt_reconnects_total = Family::<InverterLabels, Counter>::default();
        registry.register(
            "sma_bt_reconnects_total",
            "Total number of BT reconnection attempts",
            bt_reconnects_total.clone(),
        );

        let handshake_errors_total = Family::<InverterLabels, Counter>::default();
        registry.register(
            "sma_handshake_errors_total",
            "Total number of BT handshake/logon failures",
            handshake_errors_total.clone(),
        );

        let inverter_awake = Family::<InverterLabels, Gauge<i64>>::default();
        registry.register(
            "sma_inverter_awake",
            "1 when the inverter is responsive, 0 when it's asleep (BT offline)",
            inverter_awake.clone(),
        );

        let last_successful_poll_unix = Family::<InverterLabels, Gauge<i64>>::default();
        registry.register(
            "sma_last_successful_poll_unix",
            "Unix timestamp of the most recent fully successful poll cycle",
            last_successful_poll_unix.clone(),
        );

        let new_float_gauge =
            || Family::<DeviceLabels, Gauge<f64, std::sync::atomic::AtomicU64>>::default();

        let ac_power_watts = new_float_gauge();
        registry.register(
            "sma_ac_power_watts",
            "Last observed total AC output power in watts",
            ac_power_watts.clone(),
        );
        let ac_voltage_l1 = new_float_gauge();
        registry.register(
            "sma_ac_voltage_l1",
            "Last observed L1 AC grid voltage",
            ac_voltage_l1.clone(),
        );
        let ac_current_l1 = new_float_gauge();
        registry.register(
            "sma_ac_current_l1",
            "Last observed L1 AC current",
            ac_current_l1.clone(),
        );
        let grid_frequency_hz = new_float_gauge();
        registry.register(
            "sma_grid_frequency_hz",
            "Last observed grid frequency",
            grid_frequency_hz.clone(),
        );
        let dc_power_s1_watts = new_float_gauge();
        registry.register(
            "sma_dc_power_s1_watts",
            "Last observed DC string 1 power",
            dc_power_s1_watts.clone(),
        );
        let dc_power_s2_watts = new_float_gauge();
        registry.register(
            "sma_dc_power_s2_watts",
            "Last observed DC string 2 power",
            dc_power_s2_watts.clone(),
        );
        let inverter_temperature_c = new_float_gauge();
        registry.register(
            "sma_inverter_temperature_c",
            "Last observed internal inverter temperature",
            inverter_temperature_c.clone(),
        );
        let energy_today_wh = new_float_gauge();
        registry.register(
            "sma_energy_today_wh",
            "Energy produced since midnight (Wh, monotonic per day)",
            energy_today_wh.clone(),
        );
        let energy_lifetime_wh = new_float_gauge();
        registry.register(
            "sma_energy_lifetime_wh",
            "Lifetime energy produced (Wh, monotonic)",
            energy_lifetime_wh.clone(),
        );

        Self {
            registry: Arc::new(Mutex::new(registry)),
            polls_total,
            poll_errors_total,
            bt_reconnects_total,
            handshake_errors_total,
            inverter_awake,
            last_successful_poll_unix,
            ac_power_watts,
            ac_voltage_l1,
            ac_current_l1,
            grid_frequency_hz,
            dc_power_s1_watts,
            dc_power_s2_watts,
            inverter_temperature_c,
            energy_today_wh,
            energy_lifetime_wh,
        }
    }

    /// Encode the registry as Prometheus text exposition format.
    pub async fn encode(&self) -> String {
        let mut out = String::new();
        let reg = self.registry.lock().await;
        encode(&mut out, &reg).expect("encode metrics");
        out
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn encode_includes_all_families() {
        let m = MetricsRegistry::new();
        m.polls_total
            .get_or_create(&InverterLabels {
                slot: "zolder".into(),
            })
            .inc();
        m.ac_power_watts
            .get_or_create(&DeviceLabels::same("zolder"))
            .set(123.45);
        let text = m.encode().await;
        assert!(text.contains("sma_polls_total"));
        assert!(text.contains("sma_poll_errors_total"));
        assert!(text.contains("sma_bt_reconnects_total"));
        assert!(text.contains("sma_ac_power_watts"));
        assert!(text.contains(r#"slot="zolder""#));
    }

    /// MIS mode: one RFCOMM session labelled `slot=repeater`, two
    /// devices labelled `device=zolder` / `device=garage`. Per-device
    /// metrics should carry BOTH labels; session-level metrics only
    /// carry `slot`.
    #[tokio::test]
    async fn mis_labels_are_per_device() {
        let m = MetricsRegistry::new();
        m.polls_total
            .get_or_create(&InverterLabels {
                slot: "repeater".into(),
            })
            .inc();
        m.ac_power_watts
            .get_or_create(&DeviceLabels {
                slot: "repeater".into(),
                device: "zolder".into(),
            })
            .set(1500.0);
        m.ac_power_watts
            .get_or_create(&DeviceLabels {
                slot: "repeater".into(),
                device: "garage".into(),
            })
            .set(1800.0);
        let text = m.encode().await;
        assert!(text.contains(r#"slot="repeater",device="zolder""#));
        assert!(text.contains(r#"slot="repeater",device="garage""#));
        assert!(text.contains("1500"));
        assert!(text.contains("1800"));
    }
}
