//! Typed inverter readings.
//!
//! The SMA protocol encodes measurements as 28-byte records starting with an
//! LRI (Logical Record Identifier, u32) and a short "class" code, followed by
//! up to 5 values. Each value is a `(timestamp_u32, data)` pair.
//!
//! This module keeps the record parsing minimal for now — the daemon only
//! wants scalar AC power + energy totals today. Extend the `parse_*` helpers
//! as more fields are wired up.

use byteorder::{ByteOrder, LittleEndian};

/// A scalar reading tagged with an LRI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingLri {
    /// Total AC power in watts. LRI 0x263F.
    SpotPacTotal = 0x0026_3F00,
    /// Daily energy counter (Wh). LRI 0x2601.
    SpotDayEnergy = 0x0026_0100,
    /// Lifetime energy counter (Wh). LRI 0x2622.
    SpotTotalEnergy = 0x0026_2200,
    /// Inverter heatsink / internal temperature (°C × 100). LRI 0x2377.
    InverterTemperature = 0x0023_7700,
    /// Grid frequency (Hz × 100). LRI 0x4657.
    GridFrequency = 0x0046_5700,
    /// Inverter status code. LRI 0x2148.
    InverterStatus = 0x0021_4800,
}

/// Minimal structured readings pulled from a SpotACTotalPower query reply.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InverterReadings {
    pub pac_total_w: Option<i32>,
    pub day_energy_wh: Option<u64>,
    pub total_energy_wh: Option<u64>,
    pub timestamp: Option<u32>,
}

/// Parse a SpotAcTotalPower reply body (from `decode_l2`'s `rest`).
///
/// The reply is a sequence of 28-byte records. Record layout:
/// ```text
/// [0..4]   cls + LRI   (u32 LE; LRI == low 24 bits << 8, cls high byte)
/// [4..8]   timestamp   (u32 LE, seconds epoch)
/// [8..12]  value #1    (i32 LE)
/// ... up to 5 values, unused fields are 0x80000000 (int NaN) or 0xFFFFFFFF
/// ```
pub fn parse_spot_ac_total_power(body: &[u8]) -> InverterReadings {
    let mut out = InverterReadings::default();
    let stride = 28;
    let mut i = 0;
    while i + stride <= body.len() {
        let rec = &body[i..i + stride];
        let lri = LittleEndian::read_u32(&rec[0..4]);
        let ts = LittleEndian::read_u32(&rec[4..8]);
        let value = LittleEndian::read_i32(&rec[8..12]);

        // Record layout uses LRI in lower 24 bits. Mask to compare.
        let masked = lri & 0x00FF_FFFF;
        if masked == (ReadingLri::SpotPacTotal as u32 & 0x00FF_FFFF) {
            out.pac_total_w = Some(value);
            out.timestamp = Some(ts);
        }
        i += stride;
    }
    out
}

/// Parse an EnergyProduction reply — returns (day_wh, total_wh) if present.
pub fn parse_energy_production(body: &[u8]) -> (Option<u64>, Option<u64>) {
    let stride = 28;
    let mut i = 0;
    let mut day = None;
    let mut total = None;
    while i + stride <= body.len() {
        let rec = &body[i..i + stride];
        let lri = LittleEndian::read_u32(&rec[0..4]) & 0x00FF_FFFF;
        // Counters are u64 (8 bytes) at offset 8. Shift to match LE.
        let value = LittleEndian::read_u64(&rec[8..16]);
        if lri == (ReadingLri::SpotDayEnergy as u32 & 0x00FF_FFFF) {
            day = Some(value);
        } else if lri == (ReadingLri::SpotTotalEnergy as u32 & 0x00FF_FFFF) {
            total = Some(value);
        }
        i += stride;
    }
    (day, total)
}

/// Parse InverterTemperature reply (°C). Returns decimal °C.
pub fn parse_inverter_temperature(body: &[u8]) -> Option<f32> {
    let stride = 28;
    for i in (0..body.len()).step_by(stride) {
        if i + stride > body.len() {
            break;
        }
        let rec = &body[i..i + stride];
        let lri = LittleEndian::read_u32(&rec[0..4]) & 0x00FF_FFFF;
        if lri == (ReadingLri::InverterTemperature as u32 & 0x00FF_FFFF) {
            let raw = LittleEndian::read_i32(&rec[8..12]);
            return Some(raw as f32 / 100.0);
        }
    }
    None
}

/// Parse GridFrequency reply (Hz).
pub fn parse_grid_frequency(body: &[u8]) -> Option<f32> {
    let stride = 28;
    for i in (0..body.len()).step_by(stride) {
        if i + stride > body.len() {
            break;
        }
        let rec = &body[i..i + stride];
        let lri = LittleEndian::read_u32(&rec[0..4]) & 0x00FF_FFFF;
        if lri == (ReadingLri::GridFrequency as u32 & 0x00FF_FFFF) {
            let raw = LittleEndian::read_u32(&rec[8..12]);
            return Some(raw as f32 / 100.0);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_power_record() {
        // Fabricate one record: PAC total LRI, ts = 0x12345678, value = 1234 W.
        let mut rec = [0u8; 28];
        LittleEndian::write_u32(&mut rec[0..4], 0x0026_3F00);
        LittleEndian::write_u32(&mut rec[4..8], 0x12345678);
        LittleEndian::write_i32(&mut rec[8..12], 1234);
        let r = parse_spot_ac_total_power(&rec);
        assert_eq!(r.pac_total_w, Some(1234));
        assert_eq!(r.timestamp, Some(0x12345678));
    }

    #[test]
    fn empty_body_yields_default() {
        let r = parse_spot_ac_total_power(&[]);
        assert_eq!(r, InverterReadings::default());
    }

    #[test]
    fn unknown_lri_ignored() {
        let mut rec = [0u8; 28];
        LittleEndian::write_u32(&mut rec[0..4], 0x00FF_0000); // bogus LRI
        LittleEndian::write_i32(&mut rec[8..12], 999);
        let r = parse_spot_ac_total_power(&rec);
        assert!(r.pac_total_w.is_none());
    }

    #[test]
    fn parse_energy_production_both_counters() {
        let mut buf = vec![0u8; 28 * 2];
        LittleEndian::write_u32(&mut buf[0..4], 0x0026_0100);
        LittleEndian::write_u64(&mut buf[8..16], 9_072);
        LittleEndian::write_u32(&mut buf[28..32], 0x0026_2200);
        LittleEndian::write_u64(&mut buf[36..44], 39_865_677);
        let (day, total) = parse_energy_production(&buf);
        assert_eq!(day, Some(9_072));
        assert_eq!(total, Some(39_865_677));
    }

    #[test]
    fn parse_inverter_temperature_celsius() {
        let mut rec = [0u8; 28];
        LittleEndian::write_u32(&mut rec[0..4], 0x0023_7700);
        LittleEndian::write_i32(&mut rec[8..12], 3724); // 37.24 °C
        let t = parse_inverter_temperature(&rec).unwrap();
        assert!((t - 37.24).abs() < 0.001);
    }

    #[test]
    fn parse_grid_frequency_hz() {
        let mut rec = [0u8; 28];
        LittleEndian::write_u32(&mut rec[0..4], 0x0046_5700);
        LittleEndian::write_u32(&mut rec[8..12], 4998); // 49.98 Hz
        let f = parse_grid_frequency(&rec).unwrap();
        assert!((f - 49.98).abs() < 0.001);
    }
}
