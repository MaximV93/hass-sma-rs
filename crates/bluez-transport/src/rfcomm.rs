//! Real Linux BlueZ RFCOMM socket transport.
//!
//! `AF_BLUETOOTH` + `SOCK_STREAM` + `BTPROTO_RFCOMM`, channel 1.
//!
//! Implementation notes:
//!
//! - `libc` is used directly for socket syscalls because the `bluer` crate
//!   doesn't cover BT 2.0 (which HF-30 inverters are) well, and `socket2`
//!   doesn't expose `BDADDR`.
//! - The socket is placed in blocking mode for simplicity; tokio drives
//!   send/recv on a `spawn_blocking` task. RFCOMM datagrams are small and
//!   infrequent, so the overhead is negligible.
//! - Frame reassembly uses `FrameReader` — we read up to 4 KiB per `recv`,
//!   feed into the reader, and pop a complete frame when the closing
//!   delimiter arrives.

#![cfg(target_os = "linux")]

use crate::{framer::FrameReader, Result, Transport, TransportError};
use async_trait::async_trait;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task;
use tracing::{debug, warn};

/// Parse `"AA:BB:CC:DD:EE:FF"` into a little-endian 6-byte array as BlueZ
/// structs expect on the wire.
pub fn parse_bt_mac(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut out = [0u8; 6];
    for (i, p) in parts.iter().enumerate() {
        out[5 - i] = u8::from_str_radix(p, 16).ok()?;
    }
    Some(out)
}

/// Format a little-endian 6-byte BD address back to `"AA:BB:CC:DD:EE:FF"`.
pub fn format_bt_mac(addr: &[u8; 6]) -> String {
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        addr[5], addr[4], addr[3], addr[2], addr[1], addr[0]
    )
}

// BlueZ constants — not in libc crate for all versions. Values are stable
// kernel ABI.
const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_RFCOMM: libc::c_int = 3;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct SockaddrRc {
    rc_family: libc::sa_family_t,
    rc_bdaddr: [u8; 6],
    rc_channel: u8,
}

/// Real RFCOMM socket transport.
pub struct RfcommTransport {
    fd: Arc<Mutex<Option<RawFd>>>,
    reader: Arc<Mutex<FrameReader>>,
}

impl RfcommTransport {
    /// Connect to `remote_mac` on RFCOMM channel 1. `local_mac` optionally
    /// binds the socket to a specific local HCI adapter before connecting.
    pub async fn connect(
        remote_mac: [u8; 6],
        local_mac: Option<[u8; 6]>,
    ) -> Result<Self> {
        task::spawn_blocking(move || connect_blocking(remote_mac, local_mac))
            .await
            .map_err(|e| TransportError::Io(io::Error::other(e)))?
    }
}

fn connect_blocking(
    remote_mac: [u8; 6],
    local_mac: Option<[u8; 6]>,
) -> Result<RfcommTransport> {
    // SAFETY: syscall wrappers. Errors are observed via `errno` / libc's
    // returned -1 convention.
    let fd = unsafe {
        libc::socket(AF_BLUETOOTH, libc::SOCK_STREAM, BTPROTO_RFCOMM)
    };
    if fd < 0 {
        return Err(TransportError::Io(io::Error::last_os_error()));
    }

    if let Some(local) = local_mac {
        let addr = SockaddrRc {
            rc_family: AF_BLUETOOTH as libc::sa_family_t,
            rc_bdaddr: local,
            rc_channel: 1,
        };
        let rc = unsafe {
            libc::bind(
                fd,
                &addr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<SockaddrRc>() as libc::socklen_t,
            )
        };
        if rc < 0 {
            let err = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(TransportError::Io(err));
        }
    }

    let addr = SockaddrRc {
        rc_family: AF_BLUETOOTH as libc::sa_family_t,
        rc_bdaddr: remote_mac,
        rc_channel: 1,
    };
    let rc = unsafe {
        libc::connect(
            fd,
            &addr as *const _ as *const libc::sockaddr,
            std::mem::size_of::<SockaddrRc>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        let err = io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return Err(TransportError::Io(err));
    }

    // Default 15-second recv timeout; FrameReader will still honour the
    // per-call timeout below it. This backstops against a silent hang.
    let tv = libc::timeval {
        tv_sec: 15,
        tv_usec: 0,
    };
    let rc = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            &tv as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        warn!(error = %io::Error::last_os_error(), "setsockopt(SO_RCVTIMEO) failed — continuing");
    }

    debug!(
        remote = %format_bt_mac(&remote_mac),
        local  = ?local_mac.map(|m| format_bt_mac(&m)),
        "RFCOMM connected on channel 1"
    );

    Ok(RfcommTransport {
        fd: Arc::new(Mutex::new(Some(fd))),
        reader: Arc::new(Mutex::new(FrameReader::new())),
    })
}

#[async_trait]
impl Transport for RfcommTransport {
    async fn send_frame(&mut self, data: &[u8]) -> Result<usize> {
        let fd = self.fd.clone();
        let buf = data.to_vec();
        task::spawn_blocking(move || {
            let guard = fd.lock().unwrap();
            let raw = guard.ok_or(TransportError::Closed)?;
            let n = unsafe {
                libc::send(
                    raw,
                    buf.as_ptr() as *const libc::c_void,
                    buf.len(),
                    0,
                )
            };
            if n < 0 {
                Err(TransportError::Io(io::Error::last_os_error()))
            } else {
                Ok(n as usize)
            }
        })
        .await
        .map_err(|e| TransportError::Io(io::Error::other(e)))?
    }

    async fn recv_frame(&mut self, timeout_ms: u64) -> Result<Vec<u8>> {
        // Fast path: already have a complete frame buffered.
        if let Some(f) = self.reader.lock().unwrap().pop_frame() {
            return Ok(f);
        }

        let deadline = if timeout_ms > 0 {
            Some(std::time::Instant::now() + Duration::from_millis(timeout_ms))
        } else {
            None
        };

        loop {
            let fd = self.fd.clone();
            let chunk = task::spawn_blocking(move || -> Result<Vec<u8>> {
                let guard = fd.lock().unwrap();
                let raw = guard.ok_or(TransportError::Closed)?;
                let mut buf = [0u8; 4096];
                let n = unsafe {
                    libc::recv(
                        raw,
                        buf.as_mut_ptr() as *mut libc::c_void,
                        buf.len(),
                        0,
                    )
                };
                if n < 0 {
                    Err(TransportError::Io(io::Error::last_os_error()))
                } else if n == 0 {
                    Err(TransportError::Closed)
                } else {
                    Ok(buf[..n as usize].to_vec())
                }
            })
            .await
            .map_err(|e| TransportError::Io(io::Error::other(e)))??;

            self.reader.lock().unwrap().push(&chunk);
            if let Some(f) = self.reader.lock().unwrap().pop_frame() {
                return Ok(f);
            }
            if let Some(d) = deadline {
                if std::time::Instant::now() > d {
                    return Err(TransportError::Timeout { timeout_ms });
                }
            }
        }
    }

    async fn close(&mut self) -> Result<()> {
        let fd = self.fd.lock().unwrap().take();
        if let Some(raw) = fd {
            // SAFETY: fd was opened by us in connect_blocking, still valid here.
            unsafe { libc::close(raw) };
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_parse_roundtrip() {
        let mac = parse_bt_mac("00:80:25:21:32:35").unwrap();
        // Bluetooth stores in little-endian byte order.
        assert_eq!(mac, [0x35, 0x32, 0x21, 0x25, 0x80, 0x00]);
        assert_eq!(format_bt_mac(&mac), "00:80:25:21:32:35");
    }

    #[test]
    fn bad_mac_rejected() {
        assert!(parse_bt_mac("not:a:mac").is_none());
        assert!(parse_bt_mac("00:11:22:33:44").is_none());
    }
}
