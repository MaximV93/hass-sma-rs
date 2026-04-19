//! Common query commands. Each `QueryKind` maps to a `(command, first, last)`
//! triple that the inverter responds to with a range of LRI-keyed records.
//!
//! Opcode values are observed protocol constants; they are the same set any
//! inverter compatible with SMA Sunny Explorer accepts.

use crate::packet::{encode_l2, L2Header};
use byteorder::{ByteOrder, LittleEndian};

/// A scalar query the daemon understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum QueryKind {
    /// Total AC power (W). Covers LRI SPOT_PACTOT.
    SpotAcTotalPower,
    /// Per-phase AC power (W). LRI SPOT_PAC1..3.
    SpotAcPower,
    /// Per-phase AC voltage + current. LRI SPOT_UAC1..3 + SPOT_IAC1..3.
    SpotAcVoltage,
    /// Grid frequency (Hz). LRI SPOT_FREQ.
    SpotGridFrequency,
    /// Per-string DC power (W). LRI SPOT_PDC1..2.
    SpotDcPower,
    /// Per-string DC voltage + current. LRI SPOT_UDC1..2 + SPOT_IDC1..2.
    SpotDcVoltage,
    /// Energy counters: today + total kWh.
    EnergyProduction,
    /// Operation + feed-in time counters.
    OperationTime,
    /// Inverter temperature (°C).
    InverterTemperature,
    /// Inverter status (running/derating/...). LRI INV_STATUS.
    DeviceStatus,
    /// Grid relay status.
    GridRelayStatus,
    /// Inverter type, name, class.
    TypeLabel,
    /// Firmware version.
    SoftwareVersion,
    /// Battery charge status (for hybrid inverters).
    BatteryChargeStatus,
    /// Battery metadata block.
    BatteryInfo,
    /// Grid export total. LRI METERING_GRID_MS_TOT_W.
    MeteringGridTotalW,
}

impl QueryKind {
    /// Returns (command, first_lri, last_lri) tuple.
    pub const fn opcode(self) -> (u32, u32, u32) {
        use QueryKind::*;
        match self {
            SpotAcTotalPower => (0x5100_0200, 0x0026_3F00, 0x0026_3FFF),
            SpotAcPower => (0x5100_0200, 0x0046_4000, 0x0046_42FF),
            SpotAcVoltage => (0x5100_0200, 0x0046_4800, 0x0046_55FF),
            SpotGridFrequency => (0x5100_0200, 0x0046_5700, 0x0046_57FF),
            SpotDcPower => (0x5380_0200, 0x0025_1E00, 0x0025_1EFF),
            SpotDcVoltage => (0x5380_0200, 0x0045_1F00, 0x0045_21FF),
            EnergyProduction => (0x5400_0200, 0x0026_0100, 0x0026_22FF),
            OperationTime => (0x5400_0200, 0x0046_2E00, 0x0046_2FFF),
            InverterTemperature => (0x5200_0200, 0x0023_7700, 0x0023_77FF),
            DeviceStatus => (0x5180_0200, 0x0021_4800, 0x0021_48FF),
            GridRelayStatus => (0x5180_0200, 0x0041_6400, 0x0041_64FF),
            TypeLabel => (0x5800_0200, 0x0082_1E00, 0x0082_20FF),
            SoftwareVersion => (0x5800_0200, 0x0082_3400, 0x0082_34FF),
            BatteryChargeStatus => (0x5100_0200, 0x0029_5A00, 0x0029_5AFF),
            BatteryInfo => (0x5100_0200, 0x0049_1E00, 0x0049_5DFF),
            MeteringGridTotalW => (0x5100_0200, 0x0046_3600, 0x0046_3669),
        }
    }
}

/// Build the full L2 body for a range-query command.
pub fn build_query_body(
    kind: QueryKind,
    pkt_id: u16,
    app_serial: u32,
    dst_susy_id: u16,
    dst_serial: u32,
) -> Vec<u8> {
    let (cmd, first, last) = kind.opcode();
    let mut body = [0u8; 12];
    LittleEndian::write_u32(&mut body[0..4], cmd);
    LittleEndian::write_u32(&mut body[4..8], first);
    LittleEndian::write_u32(&mut body[8..12], last);

    let hdr = L2Header::query(pkt_id, app_serial, dst_susy_id, dst_serial);
    encode_l2(&hdr, &body)
}

/// Build the L2 body for an event-log query. Different wire shape
/// from spot queries: command opcode `0x7010_0200` (user-group), then
/// two u32 unix timestamps defining the time window.
///
/// - `start_unix` / `end_unix` — inclusive time window (seconds since
///   1970-01-01 UTC). Pass `(now - 86400, now)` to get the last day.
/// - Reply is multi-packet, 24-byte event records each. Use
///   `parse_event_log_records` on each fragment's command-body.
pub fn build_event_log_body(
    pkt_id: u16,
    app_serial: u32,
    dst_susy_id: u16,
    dst_serial: u32,
    start_unix: u32,
    end_unix: u32,
) -> Vec<u8> {
    let mut body = [0u8; 12];
    LittleEndian::write_u32(&mut body[0..4], 0x7010_0200);
    LittleEndian::write_u32(&mut body[4..8], start_unix);
    LittleEndian::write_u32(&mut body[8..12], end_unix);
    let hdr = L2Header::event_log_query(pkt_id, app_serial, dst_susy_id, dst_serial);
    encode_l2(&hdr, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spot_ac_total_power_has_correct_opcode() {
        let (cmd, first, last) = QueryKind::SpotAcTotalPower.opcode();
        assert_eq!(cmd, 0x5100_0200);
        assert_eq!(first, 0x0026_3F00);
        assert_eq!(last, 0x0026_3FFF);
    }

    #[test]
    fn query_body_shape() {
        let body = build_query_body(
            QueryKind::SpotAcTotalPower,
            0x0042,
            900_000_000,
            0x007D,
            0xDEAD_BEEF,
        );
        // L2 header is 28 bytes, then 12 bytes of command (3× u32 LE).
        assert_eq!(body.len(), 28 + 12);
        let cmd_slice = &body[28..];
        assert_eq!(&cmd_slice[0..4], &0x5100_0200u32.to_le_bytes());
        assert_eq!(&cmd_slice[4..8], &0x0026_3F00u32.to_le_bytes());
        assert_eq!(&cmd_slice[8..12], &0x0026_3FFFu32.to_le_bytes());
    }

    #[test]
    fn event_log_body_shape() {
        let body = build_event_log_body(0x0099, 900_000_000, 0x007D, 0xDEAD_BEEF, 1_700_000_000, 1_700_086_400);
        // Same 28-byte L2 header + 12 bytes of command body.
        assert_eq!(body.len(), 28 + 12);
        let cmd_slice = &body[28..];
        assert_eq!(&cmd_slice[0..4], &0x7010_0200u32.to_le_bytes());
        assert_eq!(&cmd_slice[4..8], &1_700_000_000u32.to_le_bytes());
        assert_eq!(&cmd_slice[8..12], &1_700_086_400u32.to_le_bytes());
        // Verify ctrl=0xE0 made it into the L2 header (byte offset 4
        // from the L2 signature start, after longwords + ctrl fields).
        // L2 signature occupies [0..4]; longwords at [4]; ctrl at [5].
        assert_eq!(body[5], 0xE0);
    }
}
