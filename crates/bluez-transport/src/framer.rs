//! Stream → frame reassembly.
//!
//! SMA BT frames on the wire start AND end with `0x7E`. `0x7E` inside the
//! payload is byte-stuffed (escaped with `0x7D 0x5E`), so we can use raw
//! delimiter scanning to split the stream.

use sma_bt_protocol::FRAME_DELIMITER;
use std::collections::VecDeque;

/// Accumulating frame reader. Feed bytes in, pull complete frames out.
#[derive(Debug, Default)]
pub struct FrameReader {
    /// Per-frame-in-progress buffer. We store the current frame's bytes here
    /// (starting with the leading `0x7E`) until we see the closing `0x7E`.
    in_progress: Vec<u8>,
    /// Whether we've seen the leading `0x7E`. Between frames we discard bytes
    /// until we see it.
    in_frame: bool,
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
            if !self.in_frame {
                if b == FRAME_DELIMITER {
                    // Start of frame. Begin collecting (including the start byte).
                    self.in_progress.clear();
                    self.in_progress.push(b);
                    self.in_frame = true;
                }
                // Otherwise: gap between frames, skip.
                continue;
            }

            self.in_progress.push(b);
            if b == FRAME_DELIMITER {
                // Closing delimiter. Complete frame.
                let done = std::mem::take(&mut self.in_progress);
                self.completed.push_back(done);
                // An immediately-following `0x7E` can serve as the next frame's
                // start — keep scanning in "between frames" mode.
                self.in_frame = false;
            }
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

    #[test]
    fn single_frame_roundtrip() {
        let mut r = FrameReader::new();
        r.push(&[0x7E, 0x01, 0x02, 0x03, 0x7E]);
        let f = r.pop_frame().unwrap();
        assert_eq!(f, [0x7E, 0x01, 0x02, 0x03, 0x7E]);
        assert!(!r.has_frame());
    }

    #[test]
    fn two_frames_back_to_back() {
        let mut r = FrameReader::new();
        r.push(&[0x7E, 0xAA, 0x7E, 0x7E, 0xBB, 0x7E]);
        let f1 = r.pop_frame().unwrap();
        let f2 = r.pop_frame().unwrap();
        assert_eq!(f1, [0x7E, 0xAA, 0x7E]);
        assert_eq!(f2, [0x7E, 0xBB, 0x7E]);
        assert!(!r.has_frame());
    }

    #[test]
    fn bytes_before_first_delimiter_dropped() {
        let mut r = FrameReader::new();
        r.push(&[0x00, 0x00, 0xFF, 0x7E, 0xAA, 0x7E]);
        assert_eq!(r.pop_frame().unwrap(), [0x7E, 0xAA, 0x7E]);
    }

    #[test]
    fn partial_frame_held_until_complete() {
        let mut r = FrameReader::new();
        r.push(&[0x7E, 0x01, 0x02]);
        assert!(!r.has_frame());
        r.push(&[0x03, 0x7E]);
        assert_eq!(r.pop_frame().unwrap(), [0x7E, 0x01, 0x02, 0x03, 0x7E]);
    }
}
