//! In-memory mock transport for testing.
//!
//! Lets tests pre-script a sequence of replies and assert what was sent.
//! Not used in production code paths.

use crate::{Result, Transport, TransportError};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// `Clone` is cheap — all handles share the same queues + sent log.
/// Lets a test keep a handle after moving one into `Session::new`.
#[derive(Clone)]
pub struct MockTransport {
    /// Canned replies the transport will return on `recv_frame`, in order.
    replies: Arc<Mutex<VecDeque<Vec<u8>>>>,
    /// Everything `send_frame` has been called with. Inspect in tests.
    pub sent: Arc<Mutex<Vec<Vec<u8>>>>,
    /// Once set, further operations return `Err(Closed)`.
    closed: Arc<Mutex<bool>>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self {
            replies: Arc::new(Mutex::new(VecDeque::new())),
            sent: Arc::new(Mutex::new(Vec::new())),
            closed: Arc::new(Mutex::new(false)),
        }
    }

    /// Queue a reply for the next `recv_frame`.
    pub fn queue_reply(&self, frame: Vec<u8>) {
        self.replies.lock().unwrap().push_back(frame);
    }

    /// Queue multiple replies in order.
    pub fn queue_replies<I: IntoIterator<Item = Vec<u8>>>(&self, iter: I) {
        let mut r = self.replies.lock().unwrap();
        for frame in iter {
            r.push_back(frame);
        }
    }

    pub fn sent_frames(&self) -> Vec<Vec<u8>> {
        self.sent.lock().unwrap().clone()
    }
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Transport for MockTransport {
    async fn send_frame(&mut self, data: &[u8]) -> Result<usize> {
        if *self.closed.lock().unwrap() {
            return Err(TransportError::Closed);
        }
        self.sent.lock().unwrap().push(data.to_vec());
        Ok(data.len())
    }

    async fn recv_frame(&mut self, _timeout_ms: u64) -> Result<Vec<u8>> {
        if *self.closed.lock().unwrap() {
            return Err(TransportError::Closed);
        }
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .ok_or(TransportError::MockExhausted)
    }

    async fn close(&mut self) -> Result<()> {
        *self.closed.lock().unwrap() = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scripted_replies() {
        let mut t = MockTransport::new();
        t.queue_reply(vec![0x7E, 0xAA, 0x7E]);
        t.queue_reply(vec![0x7E, 0xBB, 0x7E]);

        assert_eq!(t.send_frame(&[0x7E, 0x01, 0x7E]).await.unwrap(), 3);
        assert_eq!(t.recv_frame(0).await.unwrap(), vec![0x7E, 0xAA, 0x7E]);
        assert_eq!(t.recv_frame(0).await.unwrap(), vec![0x7E, 0xBB, 0x7E]);
        assert!(matches!(
            t.recv_frame(0).await,
            Err(TransportError::MockExhausted)
        ));
        assert_eq!(t.sent_frames(), vec![vec![0x7E, 0x01, 0x7E]]);
    }

    #[tokio::test]
    async fn close_makes_further_ops_error() {
        let mut t = MockTransport::new();
        t.close().await.unwrap();
        assert!(matches!(
            t.send_frame(&[]).await,
            Err(TransportError::Closed)
        ));
        assert!(matches!(t.recv_frame(0).await, Err(TransportError::Closed)));
    }
}
