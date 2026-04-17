//! Stream → frame reassembly (length-driven).
//!
//! SMA BT frames start with `0x7E` followed by a 16-bit little-endian length
//! field (`len_lo len_hi`) that counts every byte of the on-wire frame,
//! including the leading `0x7E` and — for L2-wrapped frames — the trailing
//! `0x7E` terminator.
//!
//! We **cannot** use delimiter-based scanning because L1-only frames have no
//! trailing `0x7E`: the next frame's leading `0x7E` would be swallowed into
//! the previous frame, producing an off-by-one length mismatch.
//!
//! Instead we parse the 4-byte header (`0x7E | len_lo | len_hi | hdr_cks`),
//! validate the header checksum, then consume exactly `declared_len` bytes
//! before emitting a frame.

use sma_bt_protocol::FRAME_DELIMITER;
use std::collections::VecDeque;

/// L1 header: `0x7E | len_lo | len_hi | hdr_cks` = 4 bytes.
const HEADER_LEN: usize = 4;

/// Minimum plausible declared length. A bare L1 header is 18 bytes; anything
/// shorter than that is malformed and we resync.
const MIN_FRAME_LEN: u16 = 18;

/// Safety cap: longest observed real SMA frame is ~300B. Anything larger is
/// almost certainly desync noise; drop the byte and keep scanning.
const MAX_FRAME_LEN: u16 = 2048;

/// Accumulating frame reader. Feed bytes in, pull complete frames out.
#[derive(Debug, Default)]
pub struct FrameReader {
    /// Bytes of the currently-assembling frame, starting with `0x7E`.
    in_progress: Vec<u8>,
    /// Expected total length (from header), once the header has been parsed.
    /// `None` means we haven't parsed the header yet.
    expected_len: Option<usize>,
    /// FIFO of fully-parsed frames ready to return.
    completed: VecDeque<Vec<u8>>,
}

impl FrameReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed raw bytes. Complete frames become available via [`Self::pop_frame`].
    pub fn push(&mut self, data: &[u8]) {
        for &b in data {
            self.push_byte(b);
        }
    }

    fn push_byte(&mut self, b: u8) {
        // Between frames: drop bytes until we see a 0x7E start delimiter.
        if self.in_progress.is_empty() {
            if b == FRAME_DELIMITER {
                self.in_progress.push(b);
            }
            return;
        }

        self.in_progress.push(b);

        // Header not yet parsed — once we have 4 bytes, validate it.
        if self.expected_len.is_none() {
            if self.in_progress.len() == HEADER_LEN {
                let len_lo = self.in_progress[1];
                let len_hi = self.in_progress[2];
                let declared = u16::from_le_bytes([len_lo, len_hi]);
                let hdr_cks = self.in_progress[3];
                let expected_cks = FRAME_DELIMITER ^ len_lo ^ len_hi;

                let valid = hdr_cks == expected_cks
                    && declared >= MIN_FRAME_LEN
                    && declared <= MAX_FRAME_LEN;

                if !valid {
                    // Resync: drop the leading 0x7E and replay the remaining
                    // three header bytes — one of them may be the real start
                    // of a genuine frame.
                    let tail: Vec<u8> = self.in_progress.drain(1..).collect();
                    self.in_progress.clear();
                    self.expected_len = None;
                    for b in tail {
                        self.push_byte(b);
                    }
                    return;
                }

                self.expected_len = Some(declared as usize);
            }
            return;
        }

        // We have the header; keep buffering until we hit declared length.
        let want = self.expected_len.unwrap();
        if self.in_progress.len() >= want {
            debug_assert_eq!(self.in_progress.len(), want);
            let done = std::mem::take(&mut self.in_progress);
            self.expected_len = None;
            self.completed.push_back(done);
        }
    }

    /// Pop the next complete frame if one is ready.
    pub fn pop_frame(&mut self) -> Option<Vec<u8>> {
        self.completed.pop_front()
    }

    /// Any frames queued?
    pub fn has_frame(&self) -> bool {
        !self.completed.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal 18-byte L1-only frame (header only, no payload) with
    /// correct length field + header checksum.
    fn l1_frame(payload: &[u8]) -> Vec<u8> {
        let total = 18 + payload.len();
        assert!(total <= 0xFFFF);
        let len_lo = (total & 0xFF) as u8;
        let len_hi = ((total >> 8) & 0xFF) as u8;
        let hdr_cks = FRAME_DELIMITER ^ len_lo ^ len_hi;
        let mut out = vec![FRAME_DELIMITER, len_lo, len_hi, hdr_cks];
        // 6 local + 6 dest + 2 ctrl = 14 bytes of zeros
        out.extend(std::iter::repeat(0).take(14));
        out.extend_from_slice(payload);
        assert_eq!(out.len(), total);
        out
    }

    /// Build a length-valid L2-ish frame (L1 header + arbitrary body + trailing
    /// 0x7E). The framer doesn't validate FCS or L2 shape — it just reads
    /// declared_len bytes — so this is enough to exercise length-driven logic.
    fn l2_shaped(body: &[u8]) -> Vec<u8> {
        let total = 18 + body.len() + 1;
        let len_lo = (total & 0xFF) as u8;
        let len_hi = ((total >> 8) & 0xFF) as u8;
        let hdr_cks = FRAME_DELIMITER ^ len_lo ^ len_hi;
        let mut out = vec![FRAME_DELIMITER, len_lo, len_hi, hdr_cks];
        out.extend(std::iter::repeat(0).take(14));
        out.extend_from_slice(body);
        out.push(FRAME_DELIMITER);
        assert_eq!(out.len(), total);
        out
    }

    #[test]
    fn l1_only_frame_no_trailing_delimiter() {
        // Regression for the bug that caused handshake failure: an L1-only
        // frame has no trailing 0x7E, so the old delimiter-scan swallowed the
        // next frame's start byte and produced len mismatch 31 != 32.
        let frame = l1_frame(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let mut r = FrameReader::new();
        r.push(&frame);
        let got = r.pop_frame().unwrap();
        assert_eq!(got, frame);
        assert!(!r.has_frame());
    }

    #[test]
    fn two_back_to_back_l1_frames_split_correctly() {
        let f1 = l1_frame(&[0xAA]);
        let f2 = l1_frame(&[0xBB, 0xCC]);
        let mut stream = f1.clone();
        stream.extend_from_slice(&f2);

        let mut r = FrameReader::new();
        r.push(&stream);
        assert_eq!(r.pop_frame().unwrap(), f1);
        assert_eq!(r.pop_frame().unwrap(), f2);
        assert!(!r.has_frame());
    }

    #[test]
    fn l2_shaped_frame_with_trailing_delimiter() {
        let frame = l2_shaped(&[0xFF, 0x03, 0x60, 0x65, 0x12, 0x34]);
        let mut r = FrameReader::new();
        r.push(&frame);
        assert_eq!(r.pop_frame().unwrap(), frame);
    }

    #[test]
    fn bytes_before_first_delimiter_dropped() {
        let frame = l1_frame(&[0x01, 0x02]);
        let mut stream = vec![0x00, 0x11, 0x22];
        stream.extend_from_slice(&frame);
        let mut r = FrameReader::new();
        r.push(&stream);
        assert_eq!(r.pop_frame().unwrap(), frame);
    }

    #[test]
    fn partial_frame_held_until_complete() {
        let frame = l1_frame(&[0x01, 0x02, 0x03, 0x04]);
        let mut r = FrameReader::new();
        r.push(&frame[..5]);
        assert!(!r.has_frame());
        r.push(&frame[5..]);
        assert_eq!(r.pop_frame().unwrap(), frame);
    }

    #[test]
    fn bad_header_checksum_resyncs() {
        // Start with a fake 0x7E then garbage header, then a real frame.
        // The bad-cks path should resync and find the real frame.
        let real = l1_frame(&[0xAB]);
        let mut stream = vec![FRAME_DELIMITER, 0x00, 0x00, 0xFF]; // bad cks
        stream.extend_from_slice(&real);
        let mut r = FrameReader::new();
        r.push(&stream);
        assert_eq!(r.pop_frame().unwrap(), real);
    }
}
