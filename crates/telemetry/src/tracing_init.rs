//! tracing-subscriber setup. JSON layer for production, compact for dev.

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialise the global tracing subscriber. If `RUST_LOG` is unset, falls
/// back to `info`. When `compact=true`, use human-readable output instead
/// of JSON (useful locally; production should keep JSON).
pub fn init_tracing(compact: bool) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry().with(filter);
    if compact {
        registry.with(fmt::layer().compact()).init();
    } else {
        registry.with(fmt::layer().json()).init();
    }
}
