//! BlueZ RFCOMM transport for the SMA BT protocol.
//!
//! This crate provides an async [`Transport`] trait with two implementations:
//!
//! - [`RfcommTransport`] (Linux only) — real `AF_BLUETOOTH` + `BTPROTO_RFCOMM`
//!   socket connected on channel 1. Writes are single-shot `send(2)` and reads
//!   use a blocking `recv(2)` on a tokio blocking task with the frame-delimiter
//!   driven framer.
//! - [`MockTransport`] — in-memory `VecDeque` that records sends and serves
//!   pre-canned receive payloads. Used by integration tests and by local
//!   integration tests on non-Linux developer machines.
//!
//! Frame-level reassembly (tokenising by `0x7E`) lives in [`FrameReader`]
//! so it's reused across both impls.

pub mod framer;
pub mod mock;

#[cfg(target_os = "linux")]
pub mod rfcomm;

use async_trait::async_trait;
use std::io;
use thiserror::Error;

pub use framer::FrameReader;
pub use mock::MockTransport;

#[cfg(target_os = "linux")]
pub use rfcomm::RfcommTransport;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("connection closed by peer")]
    Closed,

    #[error("frame read timed out after {timeout_ms} ms")]
    Timeout { timeout_ms: u64 },

    #[error("mock: no scripted response")]
    MockExhausted,
}

pub type Result<T> = std::result::Result<T, TransportError>;

#[async_trait]
pub trait Transport: Send + Sync {
    /// Send one fully-formed frame. Returns bytes sent.
    async fn send_frame(&mut self, data: &[u8]) -> Result<usize>;

    /// Receive the next complete frame from the peer.
    ///
    /// `timeout_ms = 0` means no timeout.
    async fn recv_frame(&mut self, timeout_ms: u64) -> Result<Vec<u8>>;

    /// Close the transport.
    async fn close(&mut self) -> Result<()>;
}
