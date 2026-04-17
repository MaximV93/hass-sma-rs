//! Layer-2 (SMA) packet body construction.
//!
//! The L2 packet lives inside the L1 Bluetooth frame. It carries:
//!
//! 1. A 4-byte "BTH L2 signature" (0x656003FF, little-endian: FF 03 60 65)
//! 2. A 24-byte L2 header (longwords, ctrl, SUSyIDs, Serials, ctrl2 words,
//!    two reserved shorts, packet id)
//! 3. A variable command-specific body
//!
//! Frame byte-stuffing and FCS checksum are handled by `FrameBuilder` in
//! `frame.rs`; this module produces the *raw* (unstuffed) payload bytes.

use crate::constants::{APP_PACKET_BIT, APP_SUSY_ID, BT_L2_SIGNATURE};
use byteorder::{ByteOrder, LittleEndian};

/// Fields of the SMA L2 header that vary per command.
#[derive(Debug, Clone, Copy)]
pub struct L2Header {
    /// "longwords" — number of 4-byte words in the packet counted in a
    /// command-specific way. Upstream uses 0x09, 0x0E, etc. depending on
    /// command; treat this as part of the command contract, not a length.
    pub longwords: u8,
    /// Control byte (0xA0, 0xA1, 0xE0 for standard commands).
    pub ctrl: u8,
    /// Destination SMA SUSyID (inverter family id), little-endian u16.
    pub dst_susy_id: u16,
    /// Destination serial number.
    pub dst_serial: u32,
    /// ctrl2 short, repeated twice in the header.
    pub ctrl2: u16,
    /// Our SUSyID — typically `APP_SUSY_ID` (125).
    pub app_susy_id: u16,
    /// Our application serial (session id); randomise per logOn.
    pub app_serial: u32,
    /// 16-bit packet id (low 15 bits). Will be or'd with `APP_PACKET_BIT`.
    pub pkt_id: u16,
}

/// Build a complete L2 body into `buf`.
///
/// Layout:
/// ```text
/// [0..4]    BT L2 signature (FF 03 60 65)
/// [4]       longwords
/// [5]       ctrl
/// [6..8]    dst_susy_id          (LE u16)
/// [8..12]   dst_serial           (LE u32)
/// [12..14]  ctrl2                (LE u16)
/// [14..16]  app_susy_id          (LE u16)
/// [16..20]  app_serial           (LE u32)
/// [20..22]  ctrl2 (repeat)       (LE u16)
/// [22..24]  0x0000
/// [24..26]  0x0000
/// [26..28]  pkt_id | 0x8000      (LE u16)
/// [28..]    command body
/// ```
pub fn encode_l2(header: &L2Header, body: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(28 + body.len());
    buf.extend_from_slice(&BT_L2_SIGNATURE.to_le_bytes());
    buf.push(header.longwords);
    buf.push(header.ctrl);
    let mut tmp = [0u8; 4];
    LittleEndian::write_u16(&mut tmp[..2], header.dst_susy_id);
    buf.extend_from_slice(&tmp[..2]);
    LittleEndian::write_u32(&mut tmp, header.dst_serial);
    buf.extend_from_slice(&tmp);
    LittleEndian::write_u16(&mut tmp[..2], header.ctrl2);
    buf.extend_from_slice(&tmp[..2]);
    LittleEndian::write_u16(&mut tmp[..2], header.app_susy_id);
    buf.extend_from_slice(&tmp[..2]);
    LittleEndian::write_u32(&mut tmp, header.app_serial);
    buf.extend_from_slice(&tmp);
    LittleEndian::write_u16(&mut tmp[..2], header.ctrl2);
    buf.extend_from_slice(&tmp[..2]);
    buf.extend_from_slice(&[0, 0, 0, 0]); // two reserved shorts
    LittleEndian::write_u16(&mut tmp[..2], header.pkt_id | APP_PACKET_BIT);
    buf.extend_from_slice(&tmp[..2]);
    buf.extend_from_slice(body);
    buf
}

/// Parse an L2 header from the start of a received payload.
///
/// Returns the header and a slice pointing at the command body. Returns
/// `None` if `data` is shorter than 28 bytes or the L2 signature does not match.
pub fn decode_l2(data: &[u8]) -> Option<(L2Header, &[u8])> {
    if data.len() < 28 {
        return None;
    }
    if LittleEndian::read_u32(&data[0..4]) != BT_L2_SIGNATURE {
        return None;
    }
    let header = L2Header {
        longwords: data[4],
        ctrl: data[5],
        dst_susy_id: LittleEndian::read_u16(&data[6..8]),
        dst_serial: LittleEndian::read_u32(&data[8..12]),
        ctrl2: LittleEndian::read_u16(&data[12..14]),
        app_susy_id: LittleEndian::read_u16(&data[14..16]),
        app_serial: LittleEndian::read_u32(&data[16..20]),
        // ctrl2 repeat at [20..22] is discarded
        // reserved shorts at [22..26] are skipped
        pkt_id: LittleEndian::read_u16(&data[26..28]) & !APP_PACKET_BIT,
    };
    Some((header, &data[28..]))
}

impl L2Header {
    /// Header for the initial BT discovery "ver" packet (cmd 0x0201). All
    /// wildcards because we don't yet know the inverter's SUSyID/serial.
    pub fn discovery(pkt_id: u16, app_serial: u32) -> Self {
        Self {
            longwords: 0x00,
            ctrl: 0x00,
            dst_susy_id: crate::constants::ANY_SUSY_ID,
            dst_serial: crate::constants::ANY_SERIAL,
            ctrl2: 0x0000,
            app_susy_id: APP_SUSY_ID,
            app_serial,
            pkt_id,
        }
    }

    /// Header for the logon packet.
    pub fn logon(pkt_id: u16, app_serial: u32) -> Self {
        Self {
            longwords: 0x0E,
            ctrl: 0xA0,
            dst_susy_id: crate::constants::ANY_SUSY_ID,
            dst_serial: crate::constants::ANY_SERIAL,
            ctrl2: 0x0100,
            app_susy_id: APP_SUSY_ID,
            app_serial,
            pkt_id,
        }
    }

    /// Generic query header. `ctrl=0xA0`, `ctrl2=0x0000`.
    pub fn query(pkt_id: u16, app_serial: u32, dst_susy_id: u16, dst_serial: u32) -> Self {
        Self {
            longwords: 0x09,
            ctrl: 0xA0,
            dst_susy_id,
            dst_serial,
            ctrl2: 0x0000,
            app_susy_id: APP_SUSY_ID,
            app_serial,
            pkt_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{ANY_SERIAL, ANY_SUSY_ID};

    #[test]
    fn encode_contains_signature_at_offset_0() {
        let hdr = L2Header::query(1, 0x1234_5678, 0x007D, 0xDEAD_BEEF);
        let out = encode_l2(&hdr, &[]);
        assert_eq!(&out[..4], &[0xFF, 0x03, 0x60, 0x65]);
    }

    #[test]
    fn encode_decode_roundtrip() {
        let hdr = L2Header {
            longwords: 0x0E,
            ctrl: 0xA0,
            dst_susy_id: ANY_SUSY_ID,
            dst_serial: ANY_SERIAL,
            ctrl2: 0x0100,
            app_susy_id: APP_SUSY_ID,
            app_serial: 900_123_456,
            pkt_id: 0x0042,
        };
        let body = [0x0C, 0x04, 0xFD, 0xFF, 0x00, 0x00, 0x00, 0x00];
        let raw = encode_l2(&hdr, &body);

        let (decoded, rest) = decode_l2(&raw).expect("decodes");
        assert_eq!(decoded.longwords, hdr.longwords);
        assert_eq!(decoded.ctrl, hdr.ctrl);
        assert_eq!(decoded.dst_susy_id, hdr.dst_susy_id);
        assert_eq!(decoded.dst_serial, hdr.dst_serial);
        assert_eq!(decoded.ctrl2, hdr.ctrl2);
        assert_eq!(decoded.app_susy_id, hdr.app_susy_id);
        assert_eq!(decoded.app_serial, hdr.app_serial);
        assert_eq!(decoded.pkt_id, hdr.pkt_id);
        assert_eq!(rest, &body);
    }

    #[test]
    fn decode_rejects_wrong_signature() {
        let mut data = vec![0u8; 28];
        data[0..4].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        assert!(decode_l2(&data).is_none());
    }

    #[test]
    fn decode_short_is_none() {
        assert!(decode_l2(&[0; 10]).is_none());
    }
}
