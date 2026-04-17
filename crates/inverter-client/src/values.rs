//! Typed inverter readings — record parsing for SMA query replies.
//!
//! Validated against live capture (0.1.13) of the zolder inverter.
//!
//! ## Reply body layout
//!
//! A query reply's L2 cmd body is:
//! ```text
//! [0..12]  12-byte prefix — opcode echo + LRI range echo (skip)
//! [12..]   sequence of records
//! ```
//!
//! ## Record layout (from SBFspot Bluetooth.cpp)
//!
//! ```text
//! [0..4]   code         dataType (1B) + LRI (2B middle bytes) + cls (1B)
//! [4..8]   datetime     u32 LE, seconds epoch
//! [8..]    value(s)     — offset + size depend on LRI / recordsize
//! ```
//!
//! Two record sizes observed:
//! - **16 bytes** — u64 counters (lifetime Wh, daily Wh). Value at `[8..16]`.
//! - **28 bytes** — i32 spot readings (power, temp, frequency, voltage).
//!   Value at `[16..20]` (offsets 8/12 carry min/max, 20..28 carry extras).
//!
//! LRI extraction: `lri = code & 0x00FF_FF00` (middle 16 bits, shifted in).
//! NaN sentinels: `0x80000000` for i32, `0xFFFFFFFF` / `0x8000000000000000`
//! for u32/u64. Treated as "no reading".

use byteorder::{ByteOrder, LittleEndian};

/// LRI constants (from SBFspot `Types.h`).
pub mod lri {
    pub const SPOT_E_TOTAL: u32 = 0x0026_0100; // MeteringTotWhOut — lifetime Wh
    pub const SPOT_E_TODAY: u32 = 0x0026_2200; // MeteringDyWhOut — today Wh
    pub const SPOT_PAC_TOTAL: u32 = 0x0026_3F00; // GridMsTotW — total AC W
    pub const INV_TEMPERATURE: u32 = 0x0023_7700; // CoolsysTmpNom — °C × 100
    pub const GRID_FREQUENCY: u32 = 0x0046_5700; // GridMsHz — Hz × 100
    pub const INV_STATUS: u32 = 0x0021_4800; // Operation status code
    pub const SW_VERSION: u32 = 0x0082_3400; // Firmware version u32
    pub const TYPE_LABEL: u32 = 0x0082_1E00; // Device type u32 index
}

/// Structured readings from a SpotAcTotalPower query.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InverterReadings {
    pub pac_total_w: Option<i32>,
    pub day_energy_wh: Option<u64>,
    pub total_energy_wh: Option<u64>,
    pub timestamp: Option<u32>,
}

/// Skip the 12-byte opcode/LRI echo prefix. Returns the record region.
/// Reply bodies are always `prefix + records`; parsing without skipping
/// would interpret the prefix as a (malformed) record.
fn records_region(body: &[u8]) -> &[u8] {
    if body.len() >= 12 {
        &body[12..]
    } else {
        &[]
    }
}

/// Extract the LRI from a record's 4-byte `code` word.
///
/// SBFspot treats the word as `dataType | LRI_high | LRI_low | cls`:
/// ```text
///   code = dataType<<24 | LRI<<8 | cls
///   LRI  = code & 0x00FF_FF00
/// ```
fn record_lri(code: u32) -> u32 {
    code & 0x00FF_FF00
}

/// i32 NaN sentinel as emitted by SMA inverters for "no reading".
const I32_NAN: i32 = -2_147_483_648; // 0x80000000
/// u32 NaN sentinel.
const U32_NAN: u32 = 0xFFFF_FFFF;
/// u64 NaN sentinel.
const U64_NAN: u64 = 0x8000_0000_0000_0000;

/// Iterate 28-byte records over the records region, calling `f` on each
/// `(lri, datetime, rec_slice)`.
fn for_each_28_record<F: FnMut(u32, u32, &[u8])>(body: &[u8], mut f: F) {
    let region = records_region(body);
    let stride = 28;
    let mut i = 0;
    while i + stride <= region.len() {
        let rec = &region[i..i + stride];
        let code = LittleEndian::read_u32(&rec[0..4]);
        let dt = LittleEndian::read_u32(&rec[4..8]);
        f(record_lri(code), dt, rec);
        i += stride;
    }
}

/// Iterate 16-byte records.
fn for_each_16_record<F: FnMut(u32, u32, &[u8])>(body: &[u8], mut f: F) {
    let region = records_region(body);
    let stride = 16;
    let mut i = 0;
    while i + stride <= region.len() {
        let rec = &region[i..i + stride];
        let code = LittleEndian::read_u32(&rec[0..4]);
        let dt = LittleEndian::read_u32(&rec[4..8]);
        f(record_lri(code), dt, rec);
        i += stride;
    }
}

/// Read the "current value" i32 from a 28-byte record (SBFspot: `recptr + 16`).
/// Returns None if it's the NaN sentinel.
fn i32_value_28(rec: &[u8]) -> Option<i32> {
    if rec.len() < 20 {
        return None;
    }
    let v = LittleEndian::read_i32(&rec[16..20]);
    if v == I32_NAN {
        None
    } else {
        Some(v)
    }
}

/// Read the u32 value at offset 16 (for unsigned records like frequency).
fn u32_value_28(rec: &[u8]) -> Option<u32> {
    if rec.len() < 20 {
        return None;
    }
    let v = LittleEndian::read_u32(&rec[16..20]);
    if v == U32_NAN || v == I32_NAN as u32 {
        None
    } else {
        Some(v)
    }
}

/// Read the u64 value at offset 8 from a 16-byte record (SBFspot: `recptr + 8`).
fn u64_value_16(rec: &[u8]) -> Option<u64> {
    if rec.len() < 16 {
        return None;
    }
    let v = LittleEndian::read_u64(&rec[8..16]);
    if v == U64_NAN || v == u64::MAX {
        None
    } else {
        Some(v)
    }
}

/// Parse a SpotAcTotalPower reply: find the SPOT_PACTOT record.
pub fn parse_spot_ac_total_power(body: &[u8]) -> InverterReadings {
    let mut out = InverterReadings::default();
    for_each_28_record(body, |lri, dt, rec| {
        if lri == lri::SPOT_PAC_TOTAL {
            out.pac_total_w = i32_value_28(rec);
            out.timestamp = Some(dt);
        }
    });
    out
}

/// Parse an EnergyProduction reply — u64 counter records at offset [8..16].
/// Returns (today_wh, lifetime_wh).
pub fn parse_energy_production(body: &[u8]) -> (Option<u64>, Option<u64>) {
    let mut today = None;
    let mut lifetime = None;
    for_each_16_record(body, |lri, _dt, rec| match lri {
        lri::SPOT_E_TODAY => today = u64_value_16(rec),
        lri::SPOT_E_TOTAL => lifetime = u64_value_16(rec),
        _ => {}
    });
    (today, lifetime)
}

/// Parse an InverterTemperature reply. Returns °C.
pub fn parse_inverter_temperature(body: &[u8]) -> Option<f32> {
    let mut temp = None;
    for_each_28_record(body, |lri, _dt, rec| {
        if lri == lri::INV_TEMPERATURE {
            temp = i32_value_28(rec).map(|v| v as f32 / 100.0);
        }
    });
    temp
}

/// Parse a GridFrequency reply. Returns Hz.
pub fn parse_grid_frequency(body: &[u8]) -> Option<f32> {
    let mut hz = None;
    for_each_28_record(body, |lri, _dt, rec| {
        if lri == lri::GRID_FREQUENCY {
            hz = u32_value_28(rec).map(|v| v as f32 / 100.0);
        }
    });
    hz
}

/// Parse a TypeLabel reply — returns the raw u32 index into SMA TagList.
pub fn parse_type_label_raw(body: &[u8]) -> Option<u32> {
    let mut out = None;
    for_each_28_record(body, |lri, _dt, rec| {
        if matches!(lri, 0x0082_1E00 | 0x0082_1F00 | 0x0082_2000) {
            out = u32_value_28(rec);
        }
    });
    out
}

/// Parse a SoftwareVersion reply: packed `(release, major, minor, build)` u32.
pub fn parse_software_version(body: &[u8]) -> Option<String> {
    let mut out = None;
    for_each_28_record(body, |lri, _dt, rec| {
        if lri == lri::SW_VERSION {
            if let Some(v) = u32_value_28(rec) {
                let build = (v & 0xFF) as u8;
                let minor = ((v >> 8) & 0xFF) as u8;
                let major = ((v >> 16) & 0xFF) as u8;
                let rel = ((v >> 24) & 0xFF) as u8;
                let rel_char = match rel {
                    0 => 'N',
                    1 => 'E',
                    2 => 'A',
                    3 => 'B',
                    4 => 'R',
                    5 => 'S',
                    _ => '?',
                };
                out = Some(format!("{:02}.{:02}.{:02}.{}", major, minor, build, rel_char));
            }
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_28_record(code: u32, dt: u32, value: i32) -> [u8; 28] {
        let mut rec = [0u8; 28];
        LittleEndian::write_u32(&mut rec[0..4], code);
        LittleEndian::write_u32(&mut rec[4..8], dt);
        // Min/max fields at [8..12] and [12..16] set to NaN.
        LittleEndian::write_i32(&mut rec[8..12], I32_NAN);
        LittleEndian::write_i32(&mut rec[12..16], I32_NAN);
        LittleEndian::write_i32(&mut rec[16..20], value);
        LittleEndian::write_i32(&mut rec[20..24], I32_NAN);
        LittleEndian::write_i32(&mut rec[24..28], I32_NAN);
        rec
    }

    fn build_16_record(code: u32, dt: u32, value: u64) -> [u8; 16] {
        let mut rec = [0u8; 16];
        LittleEndian::write_u32(&mut rec[0..4], code);
        LittleEndian::write_u32(&mut rec[4..8], dt);
        LittleEndian::write_u64(&mut rec[8..16], value);
        rec
    }

    /// Prepend the 12-byte opcode/LRI echo prefix that real replies carry.
    fn with_prefix(records: &[u8]) -> Vec<u8> {
        let mut v = Vec::with_capacity(12 + records.len());
        v.extend_from_slice(&[
            0x01, 0x02, 0x00, 0x51, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]);
        v.extend_from_slice(records);
        v
    }

    #[test]
    fn parse_single_power_record() {
        let rec = build_28_record(0x00_26_3F_01, 0x1234_5678, 1234);
        let body = with_prefix(&rec);
        let r = parse_spot_ac_total_power(&body);
        assert_eq!(r.pac_total_w, Some(1234));
        assert_eq!(r.timestamp, Some(0x1234_5678));
    }

    #[test]
    fn nan_value_yields_none() {
        let rec = build_28_record(0x00_26_3F_01, 0x1234_5678, I32_NAN);
        let body = with_prefix(&rec);
        let r = parse_spot_ac_total_power(&body);
        assert_eq!(r.pac_total_w, None);
        assert_eq!(r.timestamp, Some(0x1234_5678));
    }

    #[test]
    fn empty_body_yields_default() {
        let r = parse_spot_ac_total_power(&[]);
        assert_eq!(r, InverterReadings::default());
    }

    #[test]
    fn unknown_lri_ignored() {
        let rec = build_28_record(0x00_FF_00_01, 0, 999);
        let body = with_prefix(&rec);
        let r = parse_spot_ac_total_power(&body);
        assert!(r.pac_total_w.is_none());
    }

    #[test]
    fn parse_energy_production_both_counters() {
        let mut records = Vec::new();
        records.extend_from_slice(&build_16_record(0x00_26_01_01, 0, 39_866_198)); // E_TOTAL
        records.extend_from_slice(&build_16_record(0x00_26_22_01, 0, 10_788)); // E_TODAY
        let body = with_prefix(&records);
        let (today, total) = parse_energy_production(&body);
        assert_eq!(today, Some(10_788));
        assert_eq!(total, Some(39_866_198));
    }

    #[test]
    fn parse_inverter_temperature_celsius() {
        let rec = build_28_record(0x00_23_77_01, 0, 3_724); // 37.24 °C
        let body = with_prefix(&rec);
        let t = parse_inverter_temperature(&body).unwrap();
        assert!((t - 37.24).abs() < 0.001, "got {}", t);
    }

    #[test]
    fn parse_grid_frequency_hz() {
        let rec = build_28_record(0x00_46_57_01, 0, 4998); // 49.98 Hz
        let body = with_prefix(&rec);
        let f = parse_grid_frequency(&body).unwrap();
        assert!((f - 49.98).abs() < 0.001, "got {}", f);
    }

    #[test]
    fn parse_software_version_string() {
        let packed: i32 = ((4i32) << 24) | (2 << 16) | (30 << 8) | 6;
        let rec = build_28_record(0x00_82_34_01, 0, packed);
        let body = with_prefix(&rec);
        let v = parse_software_version(&body).unwrap();
        assert_eq!(v, "02.30.06.R");
    }

    #[test]
    fn parse_type_label_returns_raw_u32() {
        let rec = build_28_record(0x00_82_1E_01, 0, 9321);
        let body = with_prefix(&rec);
        assert_eq!(parse_type_label_raw(&body), Some(9321));
    }

    /// Regression: the real reply body observed live (nighttime — all values
    /// NaN). Parser must handle it without panicking and return None for
    /// every scalar.
    #[test]
    fn real_nighttime_nan_body_returns_none() {
        // Exact bytes captured from zolder 2026-04-17T23:22 SpotAcTotalPower
        let body: Vec<u8> = vec![
            0x01, 0x02, 0x00, 0x51, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x01, 0x3f, 0x26, 0x40, 0x84, 0x7f, 0xe2, 0x69, 0x00, 0x00, 0x00, 0x80,
            0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x80,
            0x01, 0x00, 0x00, 0x00,
        ];
        let r = parse_spot_ac_total_power(&body);
        assert_eq!(r.pac_total_w, None, "NaN must yield None not i32::MIN");
        assert_eq!(r.timestamp, Some(0x69e2_7f84));
    }
}
