//! Session state machine + session configuration.

use bluez_transport::{Transport, TransportError};
use sma_bt_protocol::{
    auth::{build_logon_body, UserGroup},
    commands::{build_query_body, QueryKind},
    frame::{Frame, FrameBuilder, ParseError},
    packet::decode_l2,
    APP_SUSY_ID,
};
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("transport: {0}")]
    Transport(#[from] TransportError),

    #[error("frame parse: {0}")]
    Parse(#[from] ParseError),

    #[error("logon failed (code 0x{code:04X})")]
    LogonFailed { code: u16 },

    #[error("inverter responded with incompatible firmware (protocol v{version})")]
    FirmwareTooOld { version: u8 },

    #[error("unexpected L2 payload shape during {phase}")]
    Protocol { phase: &'static str },

    #[error("no reply received from inverter during {phase}")]
    Silent { phase: &'static str },
}

pub type Result<T> = std::result::Result<T, SessionError>;

/// Static session configuration (per-inverter).
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Inverter BT address, little-endian byte order (as on the wire).
    pub inverter_bt: [u8; 6],
    /// Local HCI adapter's BT address, little-endian. May be zero = unknown.
    pub local_bt: [u8; 6],
    /// Password (ASCII).
    pub password: String,
    /// User group (User for 0000 default, Installer for elevated access).
    pub user_group: UserGroup,
    /// RFCOMM recv timeout in ms (per-frame).
    pub timeout_ms: u64,
    /// Multi-inverter support enabled.
    pub mis_enabled: bool,
}

/// Runtime session state. Used by callers to check/recover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Disconnected,
    Handshaking,
    Enumerating,
    LoggedIn,
    Degraded,
}

/// Live session driving a Transport.
pub struct Session<T: Transport> {
    transport: T,
    cfg: SessionConfig,
    state: SessionState,

    /// Our app-serial (random session id), chosen at construction.
    app_serial: u32,
    /// Monotonic packet id (low 15 bits).
    pcktid: u16,
    /// The inverter's SUSyID, learned on logon.
    pub inverter_susy_id: u16,
    /// The inverter's serial, learned on logon.
    pub inverter_serial: u32,
}

impl<T: Transport> Session<T> {
    pub fn new(transport: T, cfg: SessionConfig) -> Self {
        Self {
            transport,
            cfg,
            state: SessionState::Disconnected,
            app_serial: session_id(),
            pcktid: 0,
            inverter_susy_id: 0,
            inverter_serial: 0,
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    fn next_pcktid(&mut self) -> u16 {
        self.pcktid = self.pcktid.wrapping_add(1).max(1);
        self.pcktid
    }

    /// Perform the initial handshake + logon sequence. After this returns
    /// Ok, the session is LoggedIn and ready to accept queries.
    pub async fn handshake_and_logon(&mut self) -> Result<()> {
        self.state = SessionState::Handshaking;
        debug!(app_serial = self.app_serial, "starting SMA BT handshake");

        // 1. Read the inverter's spontaneous "hello" frame that it sends
        //    immediately after the RFCOMM connection is accepted.
        let hello = self
            .transport
            .recv_frame(self.cfg.timeout_ms)
            .await
            .map_err(|e| match e {
                TransportError::Timeout { .. } => {
                    SessionError::Silent { phase: "hello" }
                }
                other => other.into(),
            })?;
        let hello_frame = Frame::parse(&hello)?;
        debug!(ctrl = hello_frame.control, "got hello from inverter");

        // 2. Send the discovery "ver\r\n" command (control 0x0201) to let the
        //    inverter enumerate its NetID and firmware version.
        let pkt_id = self.next_pcktid();
        let ver_payload = b"ver\r\n";
        // L1-level command frame with a special fixed-version destination:
        let mut b = FrameBuilder::new(
            self.cfg.local_bt,
            [1, 0, 0, 0, 0, 0], // version "1.0.0"
            0x0201,
        );
        b.extend(ver_payload);
        let wire = b.build();
        self.transport.send_frame(&wire).await?;
        debug!(pkt_id, "sent ver\\r\\n");

        // 3. Read a reply and validate firmware version byte (position depends
        //    on L1 header offset, same shape as hello).
        let ver_reply = self
            .transport
            .recv_frame(self.cfg.timeout_ms)
            .await
            .map_err(|e| match e {
                TransportError::Timeout { .. } => {
                    SessionError::Silent { phase: "ver" }
                }
                other => other.into(),
            })?;
        if ver_reply.len() > 19 && ver_reply[19] < 4 {
            return Err(SessionError::FirmwareTooOld {
                version: ver_reply[19],
            });
        }

        self.state = SessionState::Enumerating;

        // 4. Logon.
        self.state = SessionState::Enumerating; // keep transient
        let now_epoch = current_epoch_u32();
        let pkt_id = self.next_pcktid();
        let body = build_logon_body(
            &self.cfg.password,
            self.cfg.user_group,
            pkt_id,
            self.app_serial,
            now_epoch,
        );
        let frame_bytes = FrameBuilder::new(
            self.cfg.local_bt,
            [0xFF; 6], // broadcast / unknown
            0x0001,
        )
        .extend(&body)
        .build();

        self.transport.send_frame(&frame_bytes).await?;
        debug!(pkt_id, now_epoch, "sent logon");

        // 5. Parse logon reply: extract SUSyID + serial + retcode.
        let logon_reply = self
            .transport
            .recv_frame(self.cfg.timeout_ms)
            .await
            .map_err(|e| match e {
                TransportError::Timeout { .. } => {
                    SessionError::Silent { phase: "logon" }
                }
                other => other.into(),
            })?;
        let logon_frame = Frame::parse(&logon_reply)?;
        let (hdr, body) = decode_l2(&logon_frame.payload)
            .ok_or(SessionError::Protocol { phase: "logon-l2" })?;

        // Retcode lives at body[0..2] in the SBFspot wire format.
        if body.len() < 2 {
            return Err(SessionError::Protocol { phase: "logon-body" });
        }
        let retcode = u16::from_le_bytes([body[0], body[1]]);
        match retcode {
            0 => {}
            0x0100 => return Err(SessionError::LogonFailed { code: 0x0100 }),
            _ => return Err(SessionError::LogonFailed { code: retcode }),
        }

        self.inverter_susy_id = hdr.app_susy_id; // dst SUSyID in reply == our dst
        self.inverter_serial = hdr.app_serial;
        self.state = SessionState::LoggedIn;
        info!(
            susy_id = self.inverter_susy_id,
            serial = self.inverter_serial,
            "logged in"
        );
        Ok(())
    }

    /// Issue a single [`QueryKind`] and return the raw response payload.
    ///
    /// Parsing typed values out of the payload is left to a higher layer
    /// (values.rs) because different queries emit different record shapes.
    pub async fn query(&mut self, kind: QueryKind) -> Result<Vec<u8>> {
        if self.state != SessionState::LoggedIn {
            return Err(SessionError::Protocol { phase: "query-not-logged-in" });
        }
        let pkt_id = self.next_pcktid();
        let body = build_query_body(
            kind,
            pkt_id,
            self.app_serial,
            self.inverter_susy_id,
            self.inverter_serial,
        );
        let frame_bytes = FrameBuilder::new(
            self.cfg.local_bt,
            [0xFF; 6], // SBFspot always uses the unknown peer here
            0x0001,
        )
        .extend(&body)
        .build();
        self.transport.send_frame(&frame_bytes).await?;
        debug!(?kind, pkt_id, "sent query");

        let reply = self
            .transport
            .recv_frame(self.cfg.timeout_ms)
            .await
            .map_err(|e| match e {
                TransportError::Timeout { .. } => SessionError::Silent { phase: "query" },
                other => other.into(),
            })?;
        let frame = Frame::parse(&reply)?;
        let (_hdr, data) = decode_l2(&frame.payload)
            .ok_or(SessionError::Protocol { phase: "query-l2" })?;
        Ok(data.to_vec())
    }

    /// Cleanly close the transport. Safe to call multiple times.
    pub async fn close(&mut self) -> Result<()> {
        self.state = SessionState::Disconnected;
        self.transport.close().await?;
        Ok(())
    }
}

fn session_id() -> u32 {
    // Same shape SBFspot uses: roughly 900M + rand to sit in the non-
    // reserved application-serial range.
    use rand::Rng;
    900_000_000u32.wrapping_add(rand::thread_rng().gen_range(0..100_000_000))
}

fn current_epoch_u32() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0)
}

// -----------------------------------------------------------------------------
// Avoid unused import warnings in the case where APP_SUSY_ID isn't directly
// referenced from this module (it's consumed via protocol-internal constants).
#[allow(dead_code)]
const _KEEP_IMPORTS: u16 = APP_SUSY_ID;
