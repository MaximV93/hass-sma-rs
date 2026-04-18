//! hass-sma-daemon entrypoint.

mod config;

use anyhow::{Context, Result};
use bluez_transport::{rfcomm::parse_bt_mac, RfcommTransport};
use clap::Parser;
use config::{DaemonConfig, InverterCfg};
use inverter_client::session::{Session, SessionConfig};
use inverter_client::values::{
    parse_device_status, parse_energy_production, parse_grid_frequency, parse_grid_relay,
    parse_inverter_temperature, parse_operation_time, parse_software_version, parse_spot_ac_power,
    parse_spot_ac_total_power, parse_spot_ac_voltage, parse_spot_dc_power, parse_spot_dc_voltage,
    status_text,
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

/// Dispatch a query reply to the right parser + MQTT publish path.
///
/// Each `QueryKind` maps to 0..N sensor keys. Silent-skip arms are queries
/// whose output we don't publish yet (e.g. archive-only data).
async fn publish_query_result(
    kind: QueryKind,
    body: &[u8],
    publisher: &DiscoveryPublisher,
    identity: &InverterIdentity,
    metrics: &MetricsRegistry,
    lbl: &InverterLabels,
) {
    match kind {
        QueryKind::SpotAcTotalPower => {
            let r = parse_spot_ac_total_power(body);
            if let Some(w) = r.pac_total_w {
                metrics.ac_power_watts.get_or_create(lbl).set(w as f64);
                let _ = publisher.publish_value(identity, "ac_power", w).await;
            }
        }
        QueryKind::SpotAcPower => {
            let r = parse_spot_ac_power(body);
            if let Some(w) = r.pac1_w {
                let _ = publisher.publish_value(identity, "ac_power_l1", w).await;
            }
            if let Some(w) = r.pac2_w {
                let _ = publisher.publish_value(identity, "ac_power_l2", w).await;
            }
            if let Some(w) = r.pac3_w {
                let _ = publisher.publish_value(identity, "ac_power_l3", w).await;
            }
        }
        QueryKind::SpotAcVoltage => {
            let r = parse_spot_ac_voltage(body);
            if let Some(v) = r.uac1_v {
                metrics.ac_voltage_l1.get_or_create(lbl).set(v as f64);
                let _ = publisher.publish_value(identity, "ac_voltage_l1", format!("{:.2}", v)).await;
            }
            if let Some(v) = r.uac2_v {
                let _ = publisher.publish_value(identity, "ac_voltage_l2", format!("{:.2}", v)).await;
            }
            if let Some(v) = r.uac3_v {
                let _ = publisher.publish_value(identity, "ac_voltage_l3", format!("{:.2}", v)).await;
            }
            if let Some(a) = r.iac1_a {
                metrics.ac_current_l1.get_or_create(lbl).set(a as f64);
                let _ = publisher.publish_value(identity, "ac_current_l1", format!("{:.3}", a)).await;
            }
            if let Some(a) = r.iac2_a {
                let _ = publisher.publish_value(identity, "ac_current_l2", format!("{:.3}", a)).await;
            }
            if let Some(a) = r.iac3_a {
                let _ = publisher.publish_value(identity, "ac_current_l3", format!("{:.3}", a)).await;
            }
        }
        QueryKind::SpotDcPower => {
            let r = parse_spot_dc_power(body);
            if let Some(w) = r.pdc1_w {
                metrics.dc_power_s1_watts.get_or_create(lbl).set(w as f64);
                let _ = publisher.publish_value(identity, "dc_power_s1", w).await;
            }
            if let Some(w) = r.pdc2_w {
                metrics.dc_power_s2_watts.get_or_create(lbl).set(w as f64);
                let _ = publisher.publish_value(identity, "dc_power_s2", w).await;
            }
        }
        QueryKind::SpotDcVoltage => {
            let r = parse_spot_dc_voltage(body);
            if let Some(v) = r.udc1_v {
                let _ = publisher.publish_value(identity, "dc_voltage_s1", format!("{:.2}", v)).await;
            }
            if let Some(v) = r.udc2_v {
                let _ = publisher.publish_value(identity, "dc_voltage_s2", format!("{:.2}", v)).await;
            }
            if let Some(a) = r.idc1_a {
                let _ = publisher.publish_value(identity, "dc_current_s1", format!("{:.3}", a)).await;
            }
            if let Some(a) = r.idc2_a {
                let _ = publisher.publish_value(identity, "dc_current_s2", format!("{:.3}", a)).await;
            }
        }
        QueryKind::EnergyProduction => {
            let (day, total) = parse_energy_production(body);
            if let Some(wh) = day {
                metrics.energy_today_wh.get_or_create(lbl).set(wh as f64);
                let _ = publisher
                    .publish_value(identity, "energy_today", format!("{:.3}", wh as f64 / 1000.0))
                    .await;
            }
            if let Some(wh) = total {
                metrics.energy_lifetime_wh.get_or_create(lbl).set(wh as f64);
                let _ = publisher
                    .publish_value(identity, "energy_lifetime", format!("{:.3}", wh as f64 / 1000.0))
                    .await;
            }
        }
        QueryKind::OperationTime => {
            let r = parse_operation_time(body);
            if let Some(s) = r.total_op_time_s {
                let _ = publisher
                    .publish_value(identity, "operation_time", format!("{:.2}", s as f64 / 3600.0))
                    .await;
            }
            if let Some(s) = r.feed_in_time_s {
                let _ = publisher
                    .publish_value(identity, "feed_in_time", format!("{:.2}", s as f64 / 3600.0))
                    .await;
            }
        }
        QueryKind::InverterTemperature => {
            if let Some(c) = parse_inverter_temperature(body) {
                metrics.inverter_temperature_c.get_or_create(lbl).set(c as f64);
                let _ = publisher.publish_value(identity, "temperature", format!("{:.2}", c)).await;
            }
        }
        QueryKind::SpotGridFrequency => {
            if let Some(hz) = parse_grid_frequency(body) {
                metrics.grid_frequency_hz.get_or_create(lbl).set(hz as f64);
                let _ = publisher.publish_value(identity, "grid_frequency", format!("{:.2}", hz)).await;
            }
        }
        QueryKind::DeviceStatus => {
            if let Some(tag) = parse_device_status(body) {
                let _ = publisher.publish_value(identity, "status", status_text(tag)).await;
            }
        }
        QueryKind::GridRelayStatus => {
            if let Some(closed) = parse_grid_relay(body) {
                let _ = publisher
                    .publish_value(identity, "grid_relay", if closed { "closed" } else { "open" })
                    .await;
            }
        }
        _ => {}
    }
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

    // Adaptive connect backoff. SMA inverters power off their BT radio
    // ~1h after sunset. Once we detect "Host is down" we likely face a
    // long sleep window — short backoff would burn BT bandwidth for
    // hours. Separate backoff ladders for transient vs persistent failures.
    const MIN_BACKOFF: Duration = Duration::from_secs(2);
    const MAX_TRANSIENT_BACKOFF: Duration = Duration::from_secs(60);
    const SLEEP_BACKOFF: Duration = Duration::from_secs(600); // 10 min when inverter asleep

    // Stable session identity: inverter tracks by app_serial. Same value
    // across reconnects → inverter sees us as the same client + accepts the
    // next logon instead of returning 0x0001 "session already active".
    let stable_app_serial: u32 = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as u32)
            .unwrap_or(900_000_000);
        // Mix the slot name's hash in so multiple inverters get distinct ids.
        let mut h = secs;
        for b in inv_cfg.slot.bytes() {
            h = h.wrapping_mul(31).wrapping_add(b as u32);
        }
        900_000_000u32.wrapping_add(h & 0x05F5_E0FF)
    };

    let mut backoff = MIN_BACKOFF;
    let mut host_down_streak: u32 = 0;
    let mut published_offline = false;
    loop {
        info!(slot = %inv_cfg.slot, "RFCOMM connect attempt");
        metrics.bt_reconnects_total.get_or_create(&lbl).inc();
        let transport = match RfcommTransport::connect(inverter_bt, local_bt).await {
            Ok(t) => {
                host_down_streak = 0;
                metrics.inverter_awake.get_or_create(&lbl).set(1);
                t
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("Host is down") || err_str.contains("os error 112") {
                    host_down_streak += 1;
                    metrics.inverter_awake.get_or_create(&lbl).set(0);
                    if host_down_streak == 3 && !published_offline {
                        let _ = publisher.publish_offline(&identity).await;
                        published_offline = true;
                        info!(
                            slot = %inv_cfg.slot,
                            "inverter appears asleep — extending reconnect to {}s",
                            SLEEP_BACKOFF.as_secs()
                        );
                    }
                }
                warn!(slot = %inv_cfg.slot, error = %e, streak = host_down_streak, "connect failed");
                let wait = if host_down_streak >= 3 {
                    SLEEP_BACKOFF
                } else {
                    backoff
                };
                tokio::time::sleep(wait).await;
                if host_down_streak < 3 {
                    backoff = (backoff * 2).min(MAX_TRANSIENT_BACKOFF);
                }
                continue;
            }
        };
        backoff = MIN_BACKOFF;

        let cfg = SessionConfig {
            inverter_bt,
            local_bt: local_bt.unwrap_or([0; 6]),
            password: inv_cfg.password.clone(),
            user_group: UserGroup::User,
            timeout_ms: rfcomm_timeout.as_millis() as u64,
            mis_enabled: inv_cfg.mis_enabled,
        };
        let mut session = Session::new_with_app_serial(transport, cfg, stable_app_serial);

        if let Err(e) = session.handshake_and_logon().await {
            metrics.handshake_errors_total.get_or_create(&lbl).inc();
            error!(slot = %inv_cfg.slot, error = %e, "handshake/logon failed");
            let _ = session.close().await;
            // Give the inverter time to fully tear down session state before
            // we reconnect. Repeated reconnects within a few seconds trigger
            // retcode 0x0001 ("session conflict") on the next logon.
            tokio::time::sleep(Duration::from_secs(5)).await;
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
        // Mark this inverter's sensors as available in HA.
        let _ = publisher.publish_online(&identity).await;
        published_offline = false;

        // Poll loop: each tick sweeps multiple QueryKinds through the same
        // persistent BT session. Queries grouped: identity (one-shot),
        // per-tick measurements (AC/DC/energy/temp/freq), diagnostics.
        //
        // Identity queries (firmware, type, status) are run ONCE after
        // logon + whenever the session reconnects, not every tick.
        let per_tick_queries = [
            QueryKind::SpotAcTotalPower,
            QueryKind::SpotAcPower,
            QueryKind::SpotAcVoltage,
            QueryKind::SpotDcPower,
            QueryKind::SpotDcVoltage,
            QueryKind::EnergyProduction,
            QueryKind::OperationTime,
            QueryKind::InverterTemperature,
            QueryKind::SpotGridFrequency,
            QueryKind::DeviceStatus,
            QueryKind::GridRelayStatus,
        ];

        // One-shot identity queries after logon.
        if let Ok(body) = session.query(QueryKind::SoftwareVersion).await {
            if let Some(ver) = parse_software_version(&body) {
                let _ = publisher.publish_value(&identity, "firmware_version", &ver).await;
                identity.firmware = ver;
            }
        }

        let mut ticker = tokio::time::interval(inv_cfg.poll_interval);
        let mut poll_count: u32 = 0;
        loop {
            ticker.tick().await;
            metrics.polls_total.get_or_create(&lbl).inc();
            poll_count += 1;

            // Parallel-run: every `yield_every` polls, drop the BT session
            // for `yield_duration` so another SMA integration can poll.
            if inv_cfg.yield_every > 0 && poll_count % inv_cfg.yield_every == 0 {
                info!(
                    slot = %inv_cfg.slot,
                    duration_secs = inv_cfg.yield_duration.as_secs(),
                    "yielding BT session for parallel-run peer"
                );
                let _ = session.close().await;
                tokio::time::sleep(inv_cfg.yield_duration).await;
                break; // outer loop reconnects
            }

            let mut cycle_ok = true;

            for kind in per_tick_queries.iter() {
                match session.query(*kind).await {
                    Ok(body) => publish_query_result(*kind, &body, &publisher, &identity, &metrics, &lbl).await,
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

            // Publish last-poll timestamp on success (ISO 8601) + update
            // metric for Grafana/alerting.
            if cycle_ok {
                let now = chrono::Utc::now();
                metrics.last_successful_poll_unix.get_or_create(&lbl).set(now.timestamp());
                let _ = publisher.publish_value(&identity, "last_poll", now.to_rfc3339()).await;
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
        // LWT topic: when this client disconnects uncleanly, broker publishes
        // "offline" here → HA flips every sensor on this inverter to unavailable.
        let lwt_topic = format!("{}/{}/availability", pub_cfg.state_prefix, inv.slot);
        let publisher = DiscoveryPublisher::connect_with_lwt(pub_cfg, Some(lwt_topic)).await;
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
