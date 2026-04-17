//! Session state machine + session configuration.

use bluez_transport::{Transport, TransportError};
use sma_bt_protocol::{
    auth::{build_init_body, build_logoff_body, build_logon_body, UserGroup},
    commands::{build_query_body, QueryKind},
    frame::{Frame, FrameBuilder, FrameKind, ParseError},
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

    /// Receive frames until we see an L1 frame with the given ctrl value.
    /// Non-matching frames (e.g. spontaneous 0x000a, 0x000c, bare 0x0006 ack
    /// frames) are logged + discarded. Mirrors SBFspot's `getPacket` which
    /// retries on command-mismatch.
    async fn recv_until_l1_ctrl(&mut self, want_ctrl: u16, phase: &'static str) -> Result<Vec<u8>> {
        for attempt in 0..12 {
            let bytes = self
                .transport
                .recv_frame(self.cfg.timeout_ms)
                .await
                .map_err(|e| match e {
                    TransportError::Timeout { .. } => SessionError::Silent { phase },
                    other => other.into(),
                })?;
            let frame = Frame::parse(&bytes)?;
            if frame.control == want_ctrl {
                return Ok(bytes);
            }
            debug!(
                attempt,
                phase,
                want = want_ctrl,
                got = frame.control,
                "skipping non-matching frame"
            );
        }
        Err(SessionError::Silent { phase })
    }

    /// Perform the initial handshake + logon sequence. After this returns
    /// Ok, the session is LoggedIn and ready to accept queries.
    ///
    /// Sequence mirrors SBFspot's `initialiseSMAConnection` + `logonSMAInverter`
    /// (single-inverter path, validated against capture 0000..0013):
    ///
    /// 1. recv hello (ctrl=0x0002). Extract NetID from byte 22, verify fw≥4.
    /// 2. send echo (ctrl=0x0002, dst=inverter, body=`0x00700400|NetID|0|1`).
    /// 3. recv topology (ctrl=0x0005). Skips any 0x000a frames in between.
    /// 4. send L2 init packet (cmd 0x0200). Inverter reply carries its
    ///    SUSyID + serial in the L2 header.
    /// 5. send L2 logoff (cmd 0xFFFD010E) — SBFspot does this pre-logon as
    ///    a session-state reset.
    /// 6. send logon (cmd 0xFFFD040C + password).
    /// 7. recv logon reply, extract retcode.
    pub async fn handshake_and_logon(&mut self) -> Result<()> {
        self.state = SessionState::Handshaking;
        debug!(app_serial = self.app_serial, "starting SMA BT handshake");

        // ── Step 1: recv hello (spontaneous broadcast from inverter)
        let hello_bytes = self
            .transport
            .recv_frame(self.cfg.timeout_ms)
            .await
            .map_err(|e| match e {
                TransportError::Timeout { .. } => SessionError::Silent { phase: "hello" },
                other => other.into(),
            })?;
        let hello = Frame::parse(&hello_bytes)?;
        if hello.control != 0x0002 {
            return Err(SessionError::Protocol { phase: "hello-ctrl" });
        }
        // Firmware protocol version at raw byte 19, NetID at byte 22.
        if hello_bytes.len() < 23 {
            return Err(SessionError::Protocol { phase: "hello-short" });
        }
        let fw = hello_bytes[19];
        if fw < 4 {
            return Err(SessionError::FirmwareTooOld { version: fw });
        }
        let net_id = hello_bytes[22];
        let inverter_bt = hello.local_bt; // src of hello = inverter's BT
        debug!(fw, net_id, ?inverter_bt, "hello parsed");

        // ── Step 2: send echo (ctrl=0x0002, L1-only, dst=inverter)
        // Body matches SBFspot: 0x00700400, NetID, 0, 1 — 13 bytes.
        let mut echo_body = Vec::with_capacity(13);
        echo_body.extend_from_slice(&0x0070_0400u32.to_le_bytes()); // 00 04 70 00
        echo_body.push(net_id);
        echo_body.extend_from_slice(&0u32.to_le_bytes());
        echo_body.extend_from_slice(&1u32.to_le_bytes());
        let echo = FrameBuilder::new_with_kind(
            FrameKind::L1Only,
            [0u8; 6], // src = zeros (matches 0002-send in capture)
            inverter_bt,
            0x0002,
        )
        .extend(&echo_body)
        .build();
        self.transport.send_frame(&echo).await?;
        debug!("echo sent");

        // ── Step 3: recv topology (ctrl=0x0005), skip any 0x000a
        let _topology_bytes = self
            .recv_until_l1_ctrl(0x0005, "topology")
            .await?;
        debug!("topology received");

        self.state = SessionState::Enumerating;

        // ── Step 4: send L2 init, recv reply with inverter SUSyID + serial
        let pkt_id = self.next_pcktid();
        let init_body = build_init_body(pkt_id, self.app_serial);
        let init_frame = FrameBuilder::new(self.cfg.local_bt, [0xFF; 6], 0x0001)
            .extend(&init_body)
            .build();
        self.transport.send_frame(&init_frame).await?;
        debug!(pkt_id, "init sent");

        let init_reply_bytes = self.recv_until_l1_ctrl(0x0001, "init").await?;
        let init_reply = Frame::parse(&init_reply_bytes)?;
        let (init_hdr, _init_cmd) = match decode_l2(&init_reply.payload) {
            Some(x) => x,
            None => {
                dump_bytes("init-reply", &init_reply_bytes, &init_reply);
                return Err(SessionError::Protocol { phase: "init-l2" });
            }
        };
        self.inverter_susy_id = init_hdr.app_susy_id;
        self.inverter_serial = init_hdr.app_serial;
        info!(
            susy_id = self.inverter_susy_id,
            serial = self.inverter_serial,
            "inverter identified"
        );

        // ── Step 5: send logoff (session reset, per SBFspot)
        let pkt_id = self.next_pcktid();
        let logoff_body = build_logoff_body(pkt_id, self.app_serial);
        let logoff_frame = FrameBuilder::new(self.cfg.local_bt, [0xFF; 6], 0x0001)
            .extend(&logoff_body)
            .build();
        self.transport.send_frame(&logoff_frame).await?;
        debug!(pkt_id, "logoff sent (pre-logon reset)");

        // ── Step 6: send logon
        let now_epoch = current_epoch_u32();
        let pkt_id = self.next_pcktid();
        let body = build_logon_body(
            &self.cfg.password,
            self.cfg.user_group,
            pkt_id,
            self.app_serial,
            now_epoch,
        );
        let frame_bytes = FrameBuilder::new(self.cfg.local_bt, [0xFF; 6], 0x0001)
            .extend(&body)
            .build();
        self.transport.send_frame(&frame_bytes).await?;
        debug!(pkt_id, now_epoch, "sent logon");

        // ── Step 7: recv logon reply (also ctrl=0x0001 at L1)
        let logon_reply = self.recv_until_l1_ctrl(0x0001, "logon").await?;
        let logon_frame = Frame::parse(&logon_reply)?;
        let (hdr, _body) = match decode_l2(&logon_frame.payload) {
            Some(x) => x,
            None => {
                dump_bytes("logon-reply", &logon_reply, &logon_frame);
                warn!("logon reply not L2 — inverter likely rejected logon shape");
                return Err(SessionError::Protocol { phase: "logon-l2" });
            }
        };

        // Retcode is in the L2 header's first reserved short (SBFspot calls
        // this `ErrorCode`, wire position L2body[22..24]). Cmd body starts
        // AFTER this field and is unused on reply.
        match hdr.error_code {
            0 => {}
            0x0100 => return Err(SessionError::LogonFailed { code: 0x0100 }),
            other => return Err(SessionError::LogonFailed { code: other }),
        }

        // Confirm the reply came from the same inverter we identified in init.
        // If SUSyID/serial differ, the reply is from a different device and
        // we should have skipped it — but don't fail, just log.
        if hdr.app_susy_id != self.inverter_susy_id || hdr.app_serial != self.inverter_serial {
            warn!(
                init_susy = self.inverter_susy_id,
                init_serial = self.inverter_serial,
                logon_susy = hdr.app_susy_id,
                logon_serial = hdr.app_serial,
                "logon reply identity mismatch"
            );
        }
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
            return Err(SessionError::Protocol {
                phase: "query-not-logged-in",
            });
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

        // Reply is L2 at L1-ctrl=0x0001. Skip any spontaneous L1 frames.
        let reply = self.recv_until_l1_ctrl(0x0001, "query").await?;
        let frame = Frame::parse(&reply)?;
        let (hdr, data) = match decode_l2(&frame.payload) {
            Some(x) => x,
            None => {
                dump_bytes("query-reply", &reply, &frame);
                return Err(SessionError::Protocol { phase: "query-l2" });
            }
        };
        let hex_body: String = data
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" ");
        debug!(
            ?kind,
            reply_susy = hdr.app_susy_id,
            reply_serial = hdr.app_serial,
            reply_pkt_id = hdr.pkt_id,
            reply_retcode = hdr.error_code,
            body_len = data.len(),
            body = %hex_body,
            "query reply"
        );
        Ok(data.to_vec())
    }

    /// Cleanly close the transport. Safe to call multiple times.
    pub async fn close(&mut self) -> Result<()> {
        self.state = SessionState::Disconnected;
        self.transport.close().await?;
        Ok(())
    }
}

/// Error-level hex dump of a raw frame and its parsed payload. Called on
/// L2 decode failures so the next iteration has byte-level evidence.
fn dump_bytes(label: &'static str, raw: &[u8], frame: &Frame) {
    let hex_raw: String = raw
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ");
    let hex_payload: String = frame
        .payload
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ");
    tracing::error!(
        label,
        raw_len = raw.len(),
        payload_len = frame.payload.len(),
        kind = ?frame.kind,
        raw = %hex_raw,
        payload = %hex_payload,
        "L2 decode failed — dumping bytes"
    );
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
