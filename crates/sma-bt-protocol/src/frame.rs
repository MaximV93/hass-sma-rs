//! Frame encoder + decoder for SMA BT Layer 1.

use crate::{constants::*, fcs::Fcs16, STUFF_BYTES, STUFF_ESCAPE, STUFF_XOR};
use thiserror::Error;

/// Minimum valid frame length: 18 header bytes + 0x7E trailer.
/// Anything shorter cannot possibly be a valid frame.
pub const MIN_FRAME_LEN: usize = 18 + 1;

/// Error returned when parsing a raw frame off the wire fails.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("frame too short: got {got} bytes, need at least {need}")]
    TooShort { got: usize, need: usize },

    #[error("missing start delimiter 0x7E (found 0x{byte:02X})")]
    MissingStart { byte: u8 },

    #[error("missing end delimiter 0x7E (found 0x{byte:02X})")]
    MissingEnd { byte: u8 },

    #[error("declared length {declared} != actual {actual}")]
    LengthMismatch { declared: u16, actual: usize },

    #[error("header checksum mismatch: got 0x{got:02X}, expected 0x{expected:02X}")]
    HeaderChecksum { got: u8, expected: u8 },

    #[error("unterminated byte-stuffing escape at end of frame")]
    TruncatedEscape,
}

/// A parsed SMA BT frame.
///
/// `local_bt` and `dest_bt` are stored as they appear on the wire (little-endian
/// BT address order, i.e. LAP first, NAP last). Convert at the transport layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub local_bt: [u8; 6],
    pub dest_bt: [u8; 6],
    pub control: u16,
    /// L2 payload AFTER un-stuffing. Includes the SMA L2 signature, headers
    /// and user command data, but *excludes* the FCS-16 trailer.
    pub payload: Vec<u8>,
}

impl Frame {
    /// Parse a single frame from a raw byte buffer.
    ///
    /// The buffer should contain exactly one complete frame. To handle
    /// stream reassembly (trailing bytes, incomplete reads), drive this from
    /// a transport-level framer that tokenises on `0x7E`.
    pub fn parse(raw: &[u8]) -> Result<Self, ParseError> {
        if raw.len() < MIN_FRAME_LEN {
            return Err(ParseError::TooShort {
                got: raw.len(),
                need: MIN_FRAME_LEN,
            });
        }
        if raw[0] != FRAME_DELIMITER {
            return Err(ParseError::MissingStart { byte: raw[0] });
        }
        if *raw.last().unwrap() != FRAME_DELIMITER {
            return Err(ParseError::MissingEnd {
                byte: *raw.last().unwrap(),
            });
        }

        let len_lo = raw[1];
        let len_hi = raw[2];
        let declared_len = u16::from_le_bytes([len_lo, len_hi]);
        let hdr_cks = raw[3];
        let expected_cks = FRAME_DELIMITER ^ len_lo ^ len_hi;
        if hdr_cks != expected_cks {
            return Err(ParseError::HeaderChecksum {
                got: hdr_cks,
                expected: expected_cks,
            });
        }
        if declared_len as usize != raw.len() {
            return Err(ParseError::LengthMismatch {
                declared: declared_len,
                actual: raw.len(),
            });
        }

        let mut local_bt = [0u8; 6];
        local_bt.copy_from_slice(&raw[4..10]);
        let mut dest_bt = [0u8; 6];
        dest_bt.copy_from_slice(&raw[10..16]);
        let control = u16::from_le_bytes([raw[16], raw[17]]);

        // Everything between header and final 0x7E is the stuffed L2 payload,
        // with the FCS-16 as the last 2 bytes (after un-stuffing).
        let stuffed = &raw[18..raw.len() - 1];
        let unstuffed = unstuff(stuffed)?;

        if unstuffed.len() < 2 {
            return Err(ParseError::TooShort {
                got: unstuffed.len(),
                need: 2,
            });
        }
        let (payload, _fcs) = unstuffed.split_at(unstuffed.len() - 2);

        Ok(Self {
            local_bt,
            dest_bt,
            control,
            payload: payload.to_vec(),
        })
    }
}

/// Build a raw wire frame.
///
/// Caller supplies the **L2 payload** (starting with the L2 signature). The
/// builder byte-stuffs, appends the FCS-16, wraps with the L1 header + trailer.
pub struct FrameBuilder {
    local_bt: [u8; 6],
    dest_bt: [u8; 6],
    control: u16,
    payload: Vec<u8>,
}

impl FrameBuilder {
    pub fn new(local_bt: [u8; 6], dest_bt: [u8; 6], control: u16) -> Self {
        Self {
            local_bt,
            dest_bt,
            control,
            payload: Vec::new(),
        }
    }

    /// Append a slice to the payload (unstuffed, raw bytes).
    pub fn extend(&mut self, bytes: &[u8]) -> &mut Self {
        self.payload.extend_from_slice(bytes);
        self
    }

    /// Serialize the frame to its on-wire bytes.
    pub fn build(&self) -> Vec<u8> {
        // FCS over the raw (unstuffed) payload.
        let fcs = Fcs16::new();
        let mut fcs = fcs;
        fcs.update_slice(&self.payload);
        let fcs_val = fcs.finalize();

        // Stuff payload + fcs (which is *also* stuffed on the wire).
        let mut stuffable = self.payload.clone();
        stuffable.push((fcs_val & 0xFF) as u8);
        stuffable.push((fcs_val >> 8) as u8);
        let stuffed = stuff(&stuffable);

        // Header (18 bytes) + stuffed payload + 0x7E trailer.
        let total_len = 18 + stuffed.len() + 1;
        let mut out = Vec::with_capacity(total_len);
        out.push(FRAME_DELIMITER);
        let len_lo = (total_len & 0xFF) as u8;
        let len_hi = ((total_len >> 8) & 0xFF) as u8;
        out.push(len_lo);
        out.push(len_hi);
        out.push(FRAME_DELIMITER ^ len_lo ^ len_hi);
        out.extend_from_slice(&self.local_bt);
        out.extend_from_slice(&self.dest_bt);
        out.push((self.control & 0xFF) as u8);
        out.push((self.control >> 8) as u8);
        out.extend_from_slice(&stuffed);
        out.push(FRAME_DELIMITER);
        out
    }
}

/// Apply byte-stuffing to `data`.
fn stuff(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 16);
    for &b in data {
        if STUFF_BYTES.contains(&b) {
            out.push(STUFF_ESCAPE);
            out.push(b ^ STUFF_XOR);
        } else {
            out.push(b);
        }
    }
    out
}

/// Remove byte-stuffing from `data`. Errors if an escape is truncated.
fn unstuff(data: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut out = Vec::with_capacity(data.len());
    let mut iter = data.iter();
    while let Some(&b) = iter.next() {
        if b == STUFF_ESCAPE {
            match iter.next() {
                Some(&n) => out.push(n ^ STUFF_XOR),
                None => return Err(ParseError::TruncatedEscape),
            }
        } else {
            out.push(b);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stuff_roundtrip_plain() {
        let data = [0x01, 0x02, 0x03];
        let s = stuff(&data);
        assert_eq!(s, data);
        assert_eq!(unstuff(&s).unwrap(), data);
    }

    #[test]
    fn stuff_escapes_control_bytes() {
        let data = [0x7E, 0x7D, 0x11, 0x12, 0x13, 0x42];
        let s = stuff(&data);
        assert_eq!(
            s,
            [0x7D, 0x5E, 0x7D, 0x5D, 0x7D, 0x31, 0x7D, 0x32, 0x7D, 0x33, 0x42]
        );
        assert_eq!(unstuff(&s).unwrap(), data);
    }

    #[test]
    fn unstuff_truncated_escape_errors() {
        assert_eq!(unstuff(&[0x7D]), Err(ParseError::TruncatedEscape));
    }

    #[test]
    fn empty_frame_too_short() {
        assert!(matches!(
            Frame::parse(&[]),
            Err(ParseError::TooShort { .. })
        ));
    }

    #[test]
    fn builder_parse_roundtrip_simple() {
        // Minimal payload = 4-byte L2 signature only + dummy body.
        let local: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let dest: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let mut b = FrameBuilder::new(local, dest, 0x0001);
        // Arbitrary non-control payload
        b.extend(&[0xFF, 0x03, 0x60, 0x65, 0x01, 0x02, 0x03, 0x04]);
        let wire = b.build();

        let parsed = Frame::parse(&wire).unwrap();
        assert_eq!(parsed.local_bt, local);
        assert_eq!(parsed.dest_bt, dest);
        assert_eq!(parsed.control, 0x0001);
        assert_eq!(
            parsed.payload,
            [0xFF, 0x03, 0x60, 0x65, 0x01, 0x02, 0x03, 0x04]
        );
    }

    #[test]
    fn header_checksum_detected() {
        let mut b = FrameBuilder::new([0; 6], [0; 6], 0);
        b.extend(&[0xFF, 0x03, 0x60, 0x65]);
        let mut wire = b.build();
        wire[3] ^= 0x01; // corrupt header checksum
        assert!(matches!(
            Frame::parse(&wire),
            Err(ParseError::HeaderChecksum { .. })
        ));
    }
}
