//! Observability setup.
//!
//! - `init_tracing()` — JSON-structured logs to stdout via `tracing-subscriber`.
//! - `MetricsRegistry` — wraps `prometheus-client` with the daemon's canonical
//!   counter + gauge set (poll_total, poll_errors_total, bt_reconnect_total,
//!   ac_power_watts).
//! - `serve_metrics(addr, registry)` — minimal hyper HTTP server that exposes
//!   `/metrics` in Prometheus text-exposition format.

pub mod metrics;
pub mod server;
pub mod tracing_init;

pub use metrics::MetricsRegistry;
pub use server::serve_metrics;
pub use tracing_init::init_tracing;
