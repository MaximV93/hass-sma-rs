//! hass-sma-daemon entrypoint.

mod config;

use anyhow::{Context, Result};
use bluez_transport::{rfcomm::parse_bt_mac, RfcommTransport};
use clap::Parser;
use config::{DaemonConfig, InverterCfg};
use inverter_client::session::{Session, SessionConfig};
use inverter_client::values::{
    parse_energy_production, parse_grid_frequency, parse_inverter_temperature,
    parse_spot_ac_total_power,
};
use mqtt_discovery::{DeviceKind, DiscoveryPublisher, InverterIdentity, MqttClientConfig};
use sma_bt_protocol::{auth::UserGroup, commands::QueryKind};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use telemetry::{init_tracing, metrics::InverterLabels, serve_metrics, MetricsRegistry};
use tokio::signal;
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[command(version, about = "SMA Sunny Boy BT daemon (Rust rewrite)")]
struct Cli {
    /// Path to config YAML.
    #[arg(short, long, default_value = "/data/options.yaml")]
    config: PathBuf,
}

/// Per-inverter task: connect, logon, poll until SIGTERM.
async fn run_inverter(
    inv_cfg: InverterCfg,
    local_bt: Option<[u8; 6]>,
    publisher: DiscoveryPublisher,
    rfcomm_timeout: Duration,
    metrics: MetricsRegistry,
) -> Result<()> {
    let lbl = InverterLabels {
        slot: inv_cfg.slot.clone(),
    };
    let inverter_bt = parse_bt_mac(&inv_cfg.bt_address)
        .with_context(|| format!("invalid BT address: {}", inv_cfg.bt_address))?;

    // Initial identity uses config-supplied model/firmware strings; once
    // we've completed logon + a TypeLabel/SoftwareVersion query, the
    // identity is refreshed with inverter-reported values.
    let mut identity = InverterIdentity {
        slot: inv_cfg.slot.clone(),
        serial: 0,
        model: inv_cfg.model.clone(),
        firmware: inv_cfg.firmware.clone(),
        kind: DeviceKind::SolarInverter,
    };

    // Retry connect on failure with exponential backoff capped at 60 s.
    let mut backoff = Duration::from_secs(2);
    loop {
        info!(slot = %inv_cfg.slot, "RFCOMM connect attempt");
        metrics.bt_reconnects_total.get_or_create(&lbl).inc();
        let transport = match RfcommTransport::connect(inverter_bt, local_bt).await {
            Ok(t) => t,
            Err(e) => {
                warn!(slot = %inv_cfg.slot, error = %e, "connect failed");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
                continue;
            }
        };
        backoff = Duration::from_secs(2);

        let cfg = SessionConfig {
            inverter_bt,
            local_bt: local_bt.unwrap_or([0; 6]),
            password: inv_cfg.password.clone(),
            user_group: UserGroup::User,
            timeout_ms: rfcomm_timeout.as_millis() as u64,
            mis_enabled: inv_cfg.mis_enabled,
        };
        let mut session = Session::new(transport, cfg);

        if let Err(e) = session.handshake_and_logon().await {
            error!(slot = %inv_cfg.slot, error = %e, "handshake/logon failed");
            let _ = session.close().await;
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(60));
            continue;
        }

        // Refresh identity with real inverter serial.
        identity.serial = session.inverter_serial;
        if let Err(e) = publisher.announce(&identity).await {
            warn!(slot = %inv_cfg.slot, error = %e, "discovery announce failed");
        } else {
            info!(slot = %inv_cfg.slot, serial = identity.serial, "announced");
        }

        // Poll loop: each tick sweeps multiple QueryKinds through the same
        // persistent BT session (matches the fork's SBFspot sweep, but
        // without re-opening BT per metric).
        let queries = [
            QueryKind::SpotAcTotalPower,
            QueryKind::EnergyProduction,
            QueryKind::InverterTemperature,
            QueryKind::SpotGridFrequency,
        ];
        let mut ticker = tokio::time::interval(inv_cfg.poll_interval);
        loop {
            ticker.tick().await;
            metrics.polls_total.get_or_create(&lbl).inc();
            let mut cycle_ok = true;
            for kind in queries.iter() {
                match session.query(*kind).await {
                    Ok(body) => match kind {
                        QueryKind::SpotAcTotalPower => {
                            let r = parse_spot_ac_total_power(&body);
                            if let Some(w) = r.pac_total_w {
                                metrics.ac_power_watts.get_or_create(&lbl).set(w as f64);
                                let _ = publisher.publish_value(&identity, "ac_power", w).await;
                            }
                        }
                        QueryKind::EnergyProduction => {
                            let (day, total) = parse_energy_production(&body);
                            if let Some(wh) = day {
                                let _ = publisher
                                    .publish_value(
                                        &identity,
                                        "energy_today",
                                        format!("{:.3}", wh as f64 / 1000.0),
                                    )
                                    .await;
                            }
                            if let Some(wh) = total {
                                let _ = publisher
                                    .publish_value(
                                        &identity,
                                        "energy_lifetime",
                                        format!("{:.3}", wh as f64 / 1000.0),
                                    )
                                    .await;
                            }
                        }
                        QueryKind::InverterTemperature => {
                            if let Some(c) = parse_inverter_temperature(&body) {
                                let _ = publisher
                                    .publish_value(
                                        &identity,
                                        "temperature",
                                        format!("{:.2}", c),
                                    )
                                    .await;
                            }
                        }
                        QueryKind::SpotGridFrequency => {
                            if let Some(hz) = parse_grid_frequency(&body) {
                                let _ = publisher
                                    .publish_value(
                                        &identity,
                                        "grid_frequency",
                                        format!("{:.2}", hz),
                                    )
                                    .await;
                            }
                        }
                        _ => {}
                    },
                    Err(e) => {
                        metrics.poll_errors_total.get_or_create(&lbl).inc();
                        warn!(
                            slot = %inv_cfg.slot, ?kind, error = %e,
                            "query failed — reconnecting"
                        );
                        cycle_ok = false;
                        break;
                    }
                }
            }
            if cycle_ok {
                let _ = publisher.publish_value(&identity, "status", "ok").await;
            } else {
                let _ = publisher.publish_value(&identity, "status", "error").await;
                break; // inner loop → reconnect
            }
        }
        let _ = session.close().await;
        // Outer loop iterates: reconnect.
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing(false);

    let cli = Cli::parse();
    let yaml = std::fs::read_to_string(&cli.config)
        .with_context(|| format!("read config at {}", cli.config.display()))?;
    let cfg: DaemonConfig = DaemonConfig::from_yaml(&yaml)?;
    info!(
        inverters = cfg.inverters.len(),
        mqtt_host = %cfg.mqtt.host,
        "daemon config loaded"
    );

    let local_bt = cfg
        .local_bt_address
        .as_deref()
        .and_then(parse_bt_mac);

    let mqtt_base = MqttClientConfig {
        host: cfg.mqtt.host.clone(),
        port: cfg.mqtt.port,
        user: cfg.mqtt.user.clone(),
        password: cfg.mqtt.password.clone(),
        client_id: cfg.mqtt.client_id.clone(),
        keep_alive: Duration::from_secs(30),
        discovery_prefix: cfg.mqtt.discovery_prefix.clone(),
        state_prefix: cfg.mqtt.state_prefix.clone(),
    };

    // Prometheus + /metrics endpoint.
    let metrics = MetricsRegistry::new();
    let metrics_addr = SocketAddr::from_str(&cfg.metrics_addr)
        .with_context(|| format!("invalid metrics_addr: {}", cfg.metrics_addr))?;
    {
        let m = metrics.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_metrics(metrics_addr, m).await {
                error!(error = %e, "metrics server stopped");
            }
        });
    }

    let mut tasks = Vec::new();
    for inv in cfg.inverters.iter() {
        let inv_clone = inv.clone();
        let mut pub_cfg = mqtt_base.clone();
        pub_cfg.client_id = format!("{}-{}", mqtt_base.client_id, inv.slot);
        let publisher = DiscoveryPublisher::connect(pub_cfg).await;
        let timeout = cfg.rfcomm_timeout;
        let m = metrics.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(e) = run_inverter(inv_clone, local_bt, publisher, timeout, m).await {
                error!(error = %e, "inverter task exited with error");
            }
        }));
    }

    info!("running {} inverter tasks; Ctrl+C to stop", tasks.len());
    signal::ctrl_c().await?;
    info!("shutdown requested, cancelling tasks");
    for t in tasks {
        t.abort();
    }
    Ok(())
}
