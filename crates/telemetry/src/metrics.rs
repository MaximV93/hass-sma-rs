//! Prometheus metrics registry.

use prometheus_client::{
    encoding::text::encode,
    metrics::{counter::Counter, family::Family, gauge::Gauge},
    registry::Registry,
};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Labels common to per-inverter metrics.
#[derive(
    Clone, Debug, Eq, Hash, PartialEq, prometheus_client::encoding::EncodeLabelSet,
)]
pub struct InverterLabels {
    pub slot: String,
}

/// The daemon's canonical metric set. Cheap to clone (Arc internally).
#[derive(Clone)]
pub struct MetricsRegistry {
    pub registry: Arc<Mutex<Registry>>,
    pub polls_total: Family<InverterLabels, Counter>,
    pub poll_errors_total: Family<InverterLabels, Counter>,
    pub bt_reconnects_total: Family<InverterLabels, Counter>,
    pub ac_power_watts: Family<InverterLabels, Gauge<f64, std::sync::atomic::AtomicU64>>,
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

        let ac_power_watts =
            Family::<InverterLabels, Gauge<f64, std::sync::atomic::AtomicU64>>::default();
        registry.register(
            "sma_ac_power_watts",
            "Last observed AC output power in watts",
            ac_power_watts.clone(),
        );

        Self {
            registry: Arc::new(Mutex::new(registry)),
            polls_total,
            poll_errors_total,
            bt_reconnects_total,
            ac_power_watts,
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
            .get_or_create(&InverterLabels { slot: "zolder".into() })
            .inc();
        let text = m.encode().await;
        assert!(text.contains("sma_polls_total"));
        assert!(text.contains("sma_poll_errors_total"));
        assert!(text.contains("sma_bt_reconnects_total"));
        assert!(text.contains("sma_ac_power_watts"));
        assert!(text.contains(r#"slot="zolder""#));
    }
}
