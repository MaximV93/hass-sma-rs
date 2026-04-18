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
///
/// Extraction: `lri = code & 0x00FF_FF00` (middle 16 bits shifted left by 8).
/// The low byte is `cls` (channel/class), high byte is `dataType`.
pub mod lri {
    // --- Identity / device info (28-byte records, value at [16..20]) ---
    pub const INV_STATUS: u32 = 0x0021_4800; // OperationHealth (code index)
    pub const INV_GRID_RELAY: u32 = 0x0041_6400; // OperationGriSwStt
    pub const INV_TYPE: u32 = 0x0082_1E00; // Device model
    pub const INV_CLASS: u32 = 0x0082_2A00; // Device class
    pub const SW_VERSION: u32 = 0x0082_3400; // Firmware version packed u32

    // --- Temperature (28-byte records, value × 100) ---
    pub const INV_TEMPERATURE: u32 = 0x0023_7700; // CoolsysTmpNom

    // --- DC side (28-byte records) ---
    pub const DC_MS_WATT: u32 = 0x0025_1E00; // SPOT_PDC1/2 (cls byte = string id)
    pub const DC_MS_VOL: u32 = 0x0045_1F00; // SPOT_UDC1/2 (value × 100 → V)
    pub const DC_MS_AMP: u32 = 0x0045_2100; // SPOT_IDC1/2 (value × 1000 → A)

    // --- Energy counters (16-byte records, u64 at [8..16]) ---
    pub const SPOT_E_TOTAL: u32 = 0x0026_0100; // MeteringTotWhOut — lifetime Wh
    pub const SPOT_E_TODAY: u32 = 0x0026_2200; // MeteringDyWhOut — today Wh
    pub const SPOT_OPER_TM: u32 = 0x0046_2E00; // MeteringTotOpTms — operating time (s)
    pub const SPOT_FEED_TM: u32 = 0x0046_2F00; // MeteringTotFeedTms — feed-in time (s)

    // --- AC side (28-byte records) ---
    pub const SPOT_PAC_TOTAL: u32 = 0x0026_3F00; // GridMsTotW — total AC W
    pub const GRID_MS_W_PHS_A: u32 = 0x0046_4000; // SPOT_PAC1 — L1 W
    pub const GRID_MS_W_PHS_B: u32 = 0x0046_4100; // SPOT_PAC2 — L2 W
    pub const GRID_MS_W_PHS_C: u32 = 0x0046_4200; // SPOT_PAC3 — L3 W
    pub const GRID_MS_VPHS_A: u32 = 0x0046_4800; // SPOT_UAC1 — L1 V × 100
    pub const GRID_MS_VPHS_B: u32 = 0x0046_4900; // SPOT_UAC2
    pub const GRID_MS_VPHS_C: u32 = 0x0046_4A00; // SPOT_UAC3
    pub const GRID_MS_APHS_A: u32 = 0x0046_5000; // SPOT_IAC1 — L1 A × 1000
    pub const GRID_MS_APHS_B: u32 = 0x0046_5100; // SPOT_IAC2
    pub const GRID_MS_APHS_C: u32 = 0x0046_5200; // SPOT_IAC3
    pub const GRID_FREQUENCY: u32 = 0x0046_5700; // GridMsHz — Hz × 100
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

/// Iterate spot-value records over the records region. Records are usually
/// 28 bytes but the inverter sometimes truncates the last one (observed:
/// SpotDcVoltage second record = 20 bytes, dropping the trailing flag
/// longwords). Minimum useful record is 20 bytes (code + dt + value slot).
fn for_each_28_record<F: FnMut(u32, u32, &[u8])>(body: &[u8], mut f: F) {
    let region = records_region(body);
    const MIN: usize = 20;
    const MAX: usize = 28;
    let mut i = 0;
    while i + MIN <= region.len() {
        let remaining = region.len() - i;
        let stride = if remaining >= MAX { MAX } else { MIN };
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

/// Per-phase AC readings. All values are Option because inverters may only
/// populate L1 (single-phase) or leave unused phases as NaN.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AcPerPhase {
    pub pac1_w: Option<i32>,
    pub pac2_w: Option<i32>,
    pub pac3_w: Option<i32>,
    pub timestamp: Option<u32>,
}

/// Parse a SpotAcPower reply (PAC1/2/3).
pub fn parse_spot_ac_power(body: &[u8]) -> AcPerPhase {
    let mut out = AcPerPhase::default();
    for_each_28_record(body, |lri, dt, rec| match lri {
        lri::GRID_MS_W_PHS_A => {
            out.pac1_w = i32_value_28(rec);
            out.timestamp = Some(dt);
        }
        lri::GRID_MS_W_PHS_B => out.pac2_w = i32_value_28(rec),
        lri::GRID_MS_W_PHS_C => out.pac3_w = i32_value_28(rec),
        _ => {}
    });
    out
}

/// Per-phase grid voltage (V) and current (A). SMA encodes both in the same
/// query reply (SpotAcVoltage).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AcVoltageCurrent {
    pub uac1_v: Option<f32>,
    pub uac2_v: Option<f32>,
    pub uac3_v: Option<f32>,
    pub iac1_a: Option<f32>,
    pub iac2_a: Option<f32>,
    pub iac3_a: Option<f32>,
}

/// Parse a SpotAcVoltage reply. Voltage raw value is × 100 → V, current is
/// × 1000 → A.
pub fn parse_spot_ac_voltage(body: &[u8]) -> AcVoltageCurrent {
    let mut out = AcVoltageCurrent::default();
    for_each_28_record(body, |lri, _dt, rec| {
        match lri {
            lri::GRID_MS_VPHS_A => out.uac1_v = i32_value_28(rec).map(|v| v as f32 / 100.0),
            lri::GRID_MS_VPHS_B => out.uac2_v = i32_value_28(rec).map(|v| v as f32 / 100.0),
            lri::GRID_MS_VPHS_C => out.uac3_v = i32_value_28(rec).map(|v| v as f32 / 100.0),
            lri::GRID_MS_APHS_A => out.iac1_a = i32_value_28(rec).map(|v| v as f32 / 1000.0),
            lri::GRID_MS_APHS_B => out.iac2_a = i32_value_28(rec).map(|v| v as f32 / 1000.0),
            lri::GRID_MS_APHS_C => out.iac3_a = i32_value_28(rec).map(|v| v as f32 / 1000.0),
            _ => {}
        }
    });
    out
}

/// Per-string DC readings.
///
/// SMA encodes up to two strings in the reply. The `cls` byte (low byte of
/// the record's `code` word) identifies the string: `cls=1` → string 1,
/// `cls=2` → string 2. Same LRI for both → we dispatch on cls.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DcPerString {
    pub pdc1_w: Option<i32>,
    pub pdc2_w: Option<i32>,
    pub udc1_v: Option<f32>,
    pub udc2_v: Option<f32>,
    pub idc1_a: Option<f32>,
    pub idc2_a: Option<f32>,
}

/// Parse an SpotDcPower reply — P string 1/2.
pub fn parse_spot_dc_power(body: &[u8]) -> DcPerString {
    parse_dc_block(body)
}

/// Parse an SpotDcVoltage reply — U/I string 1/2.
pub fn parse_spot_dc_voltage(body: &[u8]) -> DcPerString {
    parse_dc_block(body)
}

fn parse_dc_block(body: &[u8]) -> DcPerString {
    let mut out = DcPerString::default();
    let region = records_region(body);
    const MIN: usize = 20;
    const MAX: usize = 28;
    let mut i = 0;
    while i + MIN <= region.len() {
        let remaining = region.len() - i;
        let stride = if remaining >= MAX { MAX } else { MIN };
        let rec = &region[i..i + stride];
        let code = LittleEndian::read_u32(&rec[0..4]);
        let lri = record_lri(code);
        let cls = (code & 0xFF) as u8;
        match (lri, cls) {
            (lri::DC_MS_WATT, 1) => out.pdc1_w = i32_value_28(rec),
            (lri::DC_MS_WATT, 2) => out.pdc2_w = i32_value_28(rec),
            (lri::DC_MS_VOL, 1) => out.udc1_v = i32_value_28(rec).map(|v| v as f32 / 100.0),
            (lri::DC_MS_VOL, 2) => out.udc2_v = i32_value_28(rec).map(|v| v as f32 / 100.0),
            (lri::DC_MS_AMP, 1) => out.idc1_a = i32_value_28(rec).map(|v| v as f32 / 1000.0),
            (lri::DC_MS_AMP, 2) => out.idc2_a = i32_value_28(rec).map(|v| v as f32 / 1000.0),
            _ => {}
        }
        i += stride;
    }
    out
}

/// Operating-time counters (seconds).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OperationTime {
    pub total_op_time_s: Option<u64>,
    pub feed_in_time_s: Option<u64>,
}

/// Parse the OperationTime reply (two u64 counters).
pub fn parse_operation_time(body: &[u8]) -> OperationTime {
    let mut out = OperationTime::default();
    for_each_16_record(body, |lri, _dt, rec| match lri {
        lri::SPOT_OPER_TM => out.total_op_time_s = u64_value_16(rec),
        lri::SPOT_FEED_TM => out.feed_in_time_s = u64_value_16(rec),
        _ => {}
    });
    out
}

/// Parse a DeviceStatus reply — the operation-health tag/code index.
///
/// SBFspot encodes status as a record with FOUR possible status tag slots
/// at offsets 8, 12, 16, 20. Each slot is a u32 where the high byte's bit
/// 0 (0x01000000 mask) marks the CURRENTLY SELECTED state. The low 24
/// bits are the tag id. We scan all four slots and return the tag of the
/// selected one.
///
/// Common tag ids: 307 = "Ok", 455 = "Warning", 35 = "Error", 303 = "Off".
pub fn parse_device_status(body: &[u8]) -> Option<u32> {
    let mut out = None;
    for_each_28_record(body, |lri, _dt, rec| {
        if lri == lri::INV_STATUS && rec.len() >= 24 {
            for offset in [8usize, 12, 16, 20] {
                let v = LittleEndian::read_u32(&rec[offset..offset + 4]);
                if v == 0 || v == U32_NAN {
                    continue;
                }
                // High byte's low bit = "this slot is selected".
                if (v & 0x0100_0000) != 0 {
                    out = Some(v & 0x00FF_FFFF);
                    return;
                }
            }
            // No slot had the selected bit — fall back to first non-NaN
            // value as best-effort. SBFspot does the same in some branches.
            for offset in [8usize, 12, 16, 20] {
                let v = LittleEndian::read_u32(&rec[offset..offset + 4]);
                if v != 0 && v != U32_NAN {
                    out = Some(v & 0x00FF_FFFF);
                    return;
                }
            }
        }
    });
    out
}

/// Parse a GridRelayStatus reply. Returns true if relay is closed (feeding
/// grid), false if open. The underlying code values: 51 = closed, 311 = open.
pub fn parse_grid_relay(body: &[u8]) -> Option<bool> {
    let mut out = None;
    for_each_28_record(body, |lri, _dt, rec| {
        if lri == lri::INV_GRID_RELAY && rec.len() >= 12 {
            let code = LittleEndian::read_u32(&rec[8..12]) & 0x00FF_FFFF;
            out = match code {
                51 => Some(true),
                311 => Some(false),
                _ => None,
            };
        }
    });
    out
}

/// Map SMA inverter status tag id → human text. Subset of the SBFspot
/// TagList most likely to appear for an SB HF-30.
pub fn status_text(tag: u32) -> &'static str {
    match tag {
        307 => "Ok",
        455 => "Warning",
        35 => "Error",
        303 => "Off",
        443 => "Closed",
        1266 => "Rated current warning",
        _ => "Unknown",
    }
}

/// Parse a SoftwareVersion reply: packed u32 at record offset 24 (SBFspot
/// `get_long(recptr + 24)`). Major/minor are BCD-encoded bytes
/// (e.g. 0x30 → "30", not hex "48" or "30" as decimal).
///
/// Layout of the u32 (little-endian on wire):
///   byte0 = Vtype    (low nibble = index into "NEABRS")
///   byte1 = Vbuild   (plain decimal)
///   byte2 = Vminor   (BCD: '0'+hi, '0'+lo)
///   byte3 = Vmajor   (BCD)
///
/// Example: 0x02300604 → "02.30.06.R"
pub fn parse_software_version(body: &[u8]) -> Option<String> {
    let mut out = None;
    for_each_28_record(body, |lri, _dt, rec| {
        if lri == lri::SW_VERSION && rec.len() >= 28 {
            let v = LittleEndian::read_u32(&rec[24..28]);
            if v == U32_NAN || v == 0 {
                return;
            }
            let vtype = (v & 0xFF) as u8;
            let vbuild = ((v >> 8) & 0xFF) as u8;
            let vminor = ((v >> 16) & 0xFF) as u8;
            let vmajor = ((v >> 24) & 0xFF) as u8;
            let type_char = if (vtype as usize) < 6 {
                "NEABRS".as_bytes()[vtype as usize] as char
            } else {
                '?'
            };
            // BCD: high-nibble and low-nibble as decimal digits.
            let bcd_major = format!("{}{}", vmajor >> 4, vmajor & 0x0F);
            let bcd_minor = format!("{}{}", vminor >> 4, vminor & 0x0F);
            out = Some(format!(
                "{}.{}.{:02}.{}",
                bcd_major, bcd_minor, vbuild, type_char
            ));
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
        // Real capture from zolder: `04 06 30 02` at record offset 24.
        // vtype=0x04 → 'R', vbuild=0x06, vminor=0x30 (BCD→"30"),
        // vmajor=0x02 (BCD→"02") → "02.30.06.R".
        let mut rec = [0u8; 28];
        LittleEndian::write_u32(&mut rec[0..4], 0x00_82_34_01);
        // record offset 24 = value slot for SW version
        rec[24..28].copy_from_slice(&[0x04, 0x06, 0x30, 0x02]);
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
