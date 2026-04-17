//! hass-sma-daemon — main entrypoint.
//!
//! Staged bring-up. V5.1 (this revision) only wires tracing + exits.
//! Each subsequent revision plugs in a layer of the system:
//!
//! - V5.2: config loading + inventory from inverter list
//! - V5.3: bluez-transport + session lifecycle
//! - V5.4: query loop + MQTT publish
//! - V5.5: storage write-behind
//! - V5.6: Prometheus + OTLP exporters

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer().json())
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "hass-sma-daemon starting (V5.1 scaffold — no inverter polling yet)"
    );

    tokio::signal::ctrl_c().await?;
    tracing::info!("shutdown signal received, exiting");
    Ok(())
}
