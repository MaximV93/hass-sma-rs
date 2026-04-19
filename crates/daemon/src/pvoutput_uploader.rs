//! PVOutput.org periodic uploader — only compiled when the `pvoutput`
//! cargo feature is on. Reads the latest values from the shared
//! Prometheus registry + posts them to <https://pvoutput.org/service/r2/addstatus.jsp>.
//!
//! The daemon spawns one instance per configured PVOutput system.

use crate::pvoutput::{build_status_body, Status};
use std::time::Duration;
use telemetry::metrics::{DeviceLabels, MetricsRegistry};
use tracing::{debug, info, warn};

const ENDPOINT: &str = "https://pvoutput.org/service/r2/addstatus.jsp";

/// Config bundle for one uploader task.
pub struct UploaderTask {
    pub api_key: String,
    pub system_id: u32,
    pub device_slot: String,
    pub device_inner_slot: String,
    pub interval: Duration,
    pub metrics: MetricsRegistry,
}

/// Entry point. Spawn this with `tokio::spawn(upload_loop(task))`.
pub async fn upload_loop(task: UploaderTask) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "pvoutput: HTTP client build failed; uploader disabled");
            return;
        }
    };
    info!(
        system_id = task.system_id,
        slot = %task.device_inner_slot,
        interval_secs = task.interval.as_secs(),
        "pvoutput: uploader started"
    );
    let mut ticker = tokio::time::interval(task.interval);
    // First tick fires immediately — pause one interval so we have
    // real values to send.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        if let Some(status) = collect_status(&task) {
            let body = build_status_body(&status);
            match client
                .post(ENDPOINT)
                .header("X-Pvoutput-Apikey", &task.api_key)
                .header("X-Pvoutput-SystemId", task.system_id.to_string())
                .body(body.clone())
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    debug!(
                        system_id = task.system_id,
                        slot = %task.device_inner_slot,
                        "pvoutput: upload ok"
                    );
                }
                Ok(resp) => {
                    let code = resp.status();
                    let txt = resp.text().await.unwrap_or_default();
                    warn!(
                        system_id = task.system_id,
                        status = %code,
                        body = %txt.chars().take(200).collect::<String>(),
                        "pvoutput: upload rejected"
                    );
                }
                Err(e) => {
                    warn!(
                        system_id = task.system_id,
                        error = %e,
                        "pvoutput: upload request failed"
                    );
                }
            }
        } else {
            debug!(slot = %task.device_inner_slot, "pvoutput: no data yet, skipping");
        }
    }
}

/// Pull a `Status` from the shared metrics registry for this task's
/// device. Returns None if no observations have been recorded (first
/// startup + before first poll).
fn collect_status(task: &UploaderTask) -> Option<Status> {
    let lbl = DeviceLabels {
        slot: task.device_slot.clone(),
        device: task.device_inner_slot.clone(),
    };
    let power = task.metrics.ac_power_watts.get_or_create(&lbl).get() as u32;
    let energy = task.metrics.energy_today_wh.get_or_create(&lbl).get() as u64;
    let voltage = task.metrics.ac_voltage_l1.get_or_create(&lbl).get() as f32;
    let temp = task
        .metrics
        .inverter_temperature_c
        .get_or_create(&lbl)
        .get() as f32;
    if power == 0 && energy == 0 {
        // Uninitialized — no values observed yet.
        return None;
    }
    Some(Status {
        timestamp: chrono::Utc::now(),
        energy_today_wh: Some(energy as u32),
        power_w: Some(power),
        temperature_c: if temp > 0.0 { Some(temp) } else { None },
        voltage_v: if voltage > 0.0 { Some(voltage) } else { None },
    })
}
