//! hass-sma-daemon entrypoint.

mod config;

use anyhow::{Context, Result};
use clap::Parser;
use config::{DaemonConfig, InverterCfg};
use mqtt_discovery::{DeviceKind, DiscoveryPublisher, InverterIdentity, MqttClientConfig};
use std::path::PathBuf;
use std::time::Duration;
use tokio::signal;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Parser, Debug)]
#[command(version, about = "SMA Sunny Boy BT daemon (Rust rewrite)")]
struct Cli {
    /// Path to config YAML.
    #[arg(short, long, default_value = "/data/options.yaml")]
    config: PathBuf,
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer().json())
        .init();
}

async fn poll_loop_stub(inv_cfg: InverterCfg, publisher: DiscoveryPublisher) -> Result<()> {
    let identity = InverterIdentity {
        slot: inv_cfg.slot.clone(),
        serial: 0, // learned during logon — placeholder for now
        model: inv_cfg.model.clone(),
        firmware: inv_cfg.firmware.clone(),
        kind: DeviceKind::SolarInverter,
    };

    // Announce discovery configs.
    publisher
        .announce(&identity)
        .await
        .context("MQTT discovery announce")?;
    info!(slot = %inv_cfg.slot, "announced sensors (placeholder identity)");

    // Stub poll loop: on a real build this would drive
    // `inverter_client::Session` against the RFCOMM transport. We keep the
    // structural shape so the daemon compiles + runs even without BT.
    let mut ticker = tokio::time::interval(inv_cfg.poll_interval);
    loop {
        ticker.tick().await;
        // Emit a heartbeat status until the real poll path lands.
        if let Err(e) = publisher
            .publish_value(&identity, "status", "scaffold")
            .await
        {
            warn!(slot = %inv_cfg.slot, error = %e, "publish failed");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();
    let yaml = std::fs::read_to_string(&cli.config)
        .with_context(|| format!("read config at {}", cli.config.display()))?;
    let cfg: DaemonConfig = DaemonConfig::from_yaml(&yaml)?;
    info!(
        inverters = cfg.inverters.len(),
        mqtt_host = %cfg.mqtt.host,
        "daemon config loaded"
    );

    let mqtt_cfg = MqttClientConfig {
        host: cfg.mqtt.host.clone(),
        port: cfg.mqtt.port,
        user: cfg.mqtt.user.clone(),
        password: cfg.mqtt.password.clone(),
        client_id: cfg.mqtt.client_id.clone(),
        keep_alive: Duration::from_secs(30),
        discovery_prefix: cfg.mqtt.discovery_prefix.clone(),
        state_prefix: cfg.mqtt.state_prefix.clone(),
    };
    let publisher = DiscoveryPublisher::connect(mqtt_cfg).await;

    // Spawn a poll loop per inverter. Each loop owns its Transport + Session.
    let mut tasks = Vec::new();
    for inv in cfg.inverters.iter() {
        let inv_clone = inv.clone();
        // Clone publisher channel — ordering on single broker is fine.
        let publisher_clone = DiscoveryPublisher::connect(MqttClientConfig {
            host: cfg.mqtt.host.clone(),
            port: cfg.mqtt.port,
            user: cfg.mqtt.user.clone(),
            password: cfg.mqtt.password.clone(),
            client_id: format!("{}-{}", cfg.mqtt.client_id, inv.slot),
            keep_alive: Duration::from_secs(30),
            discovery_prefix: cfg.mqtt.discovery_prefix.clone(),
            state_prefix: cfg.mqtt.state_prefix.clone(),
        })
        .await;
        tasks.push(tokio::spawn(async move {
            if let Err(e) = poll_loop_stub(inv_clone, publisher_clone).await {
                error!(error = %e, "inverter task exited");
            }
        }));
    }
    // Use the first publisher to satisfy the compiler (we split channels).
    drop(publisher);

    info!("running {} inverter tasks; Ctrl+C to stop", tasks.len());
    signal::ctrl_c().await?;
    info!("shutdown requested, cancelling tasks");
    for t in tasks {
        t.abort();
    }
    Ok(())
}
