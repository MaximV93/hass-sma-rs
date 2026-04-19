//! PVOutput.org upload helpers.
//!
//! Status: body-builder + config present, HTTP uploader deliberately
//! NOT wired. PVOutput uses a tiny URL-encoded POST body per reading;
//! we assemble that here in a dep-free way so the daemon binary stays
//! small. A future feature-flagged module will add a reqwest/hyper
//! uploader once the user opts in.
//!
//! API reference: <https://pvoutput.org/help/api_specification.html>
//! Endpoint: `POST https://pvoutput.org/service/r2/addstatus.jsp`
//! Auth: two headers — `X-Pvoutput-Apikey` + `X-Pvoutput-SystemId`.
//! Rate limit: 60 POSTs/hour per API key.

use chrono::{DateTime, TimeZone};
use serde::{Deserialize, Serialize};

/// Per-system PVOutput configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PvoutputSystem {
    /// Which inverter slot this system reflects.
    pub slot: String,
    /// pvoutput.org SystemId (numeric).
    pub system_id: u32,
}

/// Top-level PVOutput config (one API key, N systems).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PvoutputCfg {
    /// API key from pvoutput.org account settings.
    pub api_key: String,
    /// Registered systems mapping slot → system_id.
    #[serde(default)]
    pub systems: Vec<PvoutputSystem>,
    /// Upload interval. PVOutput accepts 5, 10, 15 min statuses; 5 min
    /// is the most common. Default: 5 min.
    #[serde(with = "humantime_serde", default = "default_interval")]
    pub upload_interval: std::time::Duration,
}

fn default_interval() -> std::time::Duration {
    std::time::Duration::from_secs(300)
}

/// One reading to upload. Energy fields are cumulative since midnight
/// (PVOutput's `v1`), power is instantaneous (`v2`).
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub timestamp: DateTime<chrono::Utc>,
    pub energy_today_wh: Option<u32>,
    pub power_w: Option<u32>,
    pub temperature_c: Option<f32>,
    pub voltage_v: Option<f32>,
}

/// Build the URL-encoded POST body for an `addstatus.jsp` request.
///
/// Format: `d=YYYYMMDD&t=HH%3AMM&v1=N&v2=N[&v5=N][&v6=N]`. PVOutput
/// accepts any subset of v1..v6 — missing fields are just omitted.
/// Date + time are in the SYSTEM's local timezone per PVOutput's API
/// but we pass UTC for consistency; users set their PVOutput profile
/// to UTC to match.
pub fn build_status_body(s: &Status) -> String {
    let d = s.timestamp.format("%Y%m%d").to_string();
    let t = s.timestamp.format("%H:%M").to_string();
    let mut parts = vec![format!("d={d}"), format!("t={}", urlencode(&t))];
    if let Some(v) = s.energy_today_wh {
        parts.push(format!("v1={v}"));
    }
    if let Some(v) = s.power_w {
        parts.push(format!("v2={v}"));
    }
    if let Some(v) = s.temperature_c {
        parts.push(format!("v5={v:.1}"));
    }
    if let Some(v) = s.voltage_v {
        parts.push(format!("v6={v:.1}"));
    }
    parts.join("&")
}

/// Tiny url-encoder. Only needs to encode `:` for the HH:MM time field
/// — PVOutput's other fields are digits + decimal dots.
fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

/// Convenience: produce a `Status` from the observed HA sensor values
/// at a point in time. Missing readings (e.g. inverter asleep) yield
/// `None` for that field.
#[allow(dead_code)] // test-only for now
pub fn status_at(
    timestamp: DateTime<chrono::Utc>,
    energy_today_wh: Option<u64>,
    power_w: Option<u32>,
    temperature_c: Option<f32>,
    voltage_v: Option<f32>,
) -> Status {
    Status {
        timestamp,
        energy_today_wh: energy_today_wh.and_then(|v| u32::try_from(v).ok()),
        power_w,
        temperature_c,
        voltage_v,
    }
}

/// Helper for callers that don't yet speak DateTime: build a Status
/// from a unix timestamp directly.
#[allow(dead_code)] // test-only for now
pub fn status_from_unix(
    unix_secs: u32,
    energy_today_wh: Option<u64>,
    power_w: Option<u32>,
    temperature_c: Option<f32>,
    voltage_v: Option<f32>,
) -> Status {
    let ts = chrono::Utc.timestamp_opt(unix_secs as i64, 0).unwrap();
    status_at(ts, energy_today_wh, power_w, temperature_c, voltage_v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn body_minimal_shape() {
        let s = Status {
            timestamp: chrono::Utc
                .with_ymd_and_hms(2026, 4, 19, 13, 30, 0)
                .unwrap(),
            energy_today_wh: Some(9876),
            power_w: Some(2500),
            temperature_c: None,
            voltage_v: None,
        };
        let body = build_status_body(&s);
        assert_eq!(body, "d=20260419&t=13%3A30&v1=9876&v2=2500");
    }

    #[test]
    fn body_includes_temp_and_voltage() {
        let s = Status {
            timestamp: chrono::Utc.with_ymd_and_hms(2026, 4, 19, 9, 5, 0).unwrap(),
            energy_today_wh: Some(420),
            power_w: Some(180),
            temperature_c: Some(24.7),
            voltage_v: Some(230.12),
        };
        let body = build_status_body(&s);
        assert_eq!(body, "d=20260419&t=09%3A05&v1=420&v2=180&v5=24.7&v6=230.1");
    }

    #[test]
    fn body_omits_absent_fields() {
        let s = Status {
            timestamp: chrono::Utc.with_ymd_and_hms(2026, 4, 19, 0, 0, 0).unwrap(),
            energy_today_wh: None,
            power_w: None,
            temperature_c: None,
            voltage_v: None,
        };
        assert_eq!(build_status_body(&s), "d=20260419&t=00%3A00");
    }

    #[test]
    fn urlencode_colon_is_percent_3a() {
        assert_eq!(urlencode("12:30"), "12%3A30");
        assert_eq!(urlencode("abc-def_ghi.jkl~"), "abc-def_ghi.jkl~");
    }

    #[test]
    fn status_at_coerces_u64_to_u32() {
        let ts = chrono::Utc.with_ymd_and_hms(2026, 4, 19, 12, 0, 0).unwrap();
        let s = status_at(ts, Some(12345), Some(300), None, None);
        assert_eq!(s.energy_today_wh, Some(12345));
        // u32 overflow case: u64::MAX should coerce to None.
        let s = status_at(ts, Some(u64::MAX), Some(300), None, None);
        assert_eq!(s.energy_today_wh, None);
    }

    #[test]
    fn cfg_roundtrips_through_serde_yaml() {
        let yaml = r#"
api_key: "abc123def456"
systems:
  - slot: zolder
    system_id: 12345
  - slot: garage
    system_id: 67890
upload_interval: 5m
"#;
        let cfg: PvoutputCfg = serde_yaml::from_str(yaml).expect("parse yaml");
        assert_eq!(cfg.api_key, "abc123def456");
        assert_eq!(cfg.systems.len(), 2);
        assert_eq!(cfg.systems[0].slot, "zolder");
        assert_eq!(cfg.systems[0].system_id, 12345);
        assert_eq!(cfg.systems[1].slot, "garage");
        assert_eq!(cfg.upload_interval, std::time::Duration::from_secs(300));
        // Roundtrip back to YAML; parse again and verify stable.
        let dump = serde_yaml::to_string(&cfg).expect("encode yaml");
        let cfg2: PvoutputCfg = serde_yaml::from_str(&dump).expect("re-parse");
        assert_eq!(cfg.api_key, cfg2.api_key);
        assert_eq!(cfg.systems.len(), cfg2.systems.len());
    }

    #[test]
    fn cfg_defaults_upload_interval_to_5min() {
        let yaml = r#"
api_key: "xxx"
systems:
  - slot: zolder
    system_id: 100
"#;
        let cfg: PvoutputCfg = serde_yaml::from_str(yaml).expect("parse yaml");
        assert_eq!(cfg.upload_interval, std::time::Duration::from_secs(300));
    }

    #[test]
    fn cfg_accepts_zero_systems() {
        let yaml = "api_key: xxx\n";
        let cfg: PvoutputCfg = serde_yaml::from_str(yaml).expect("parse yaml");
        assert!(cfg.systems.is_empty());
    }
}
