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

/// Parsed L2 reply for caller consumption.
pub struct L2Reply {
    pub pkt_id: u16,
    pub error_code: u16,
    pub app_susy_id: u16,
    pub app_serial: u32,
    pub body: Vec<u8>,
}

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
        Self::new_with_app_serial(transport, cfg, session_id())
    }

    /// Build a session with an explicit `app_serial`. The inverter tracks
    /// session identity by app_serial; if we reuse the same one across
    /// reconnects the inverter recognises us as the same client and
    /// doesn't reject our logon with 0x0001 ("session already active").
    pub fn new_with_app_serial(transport: T, cfg: SessionConfig, app_serial: u32) -> Self {
        Self {
            transport,
            cfg,
            state: SessionState::Disconnected,
            app_serial,
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
            let frame = match Frame::parse(&bytes) {
                Ok(f) => f,
                Err(e) => {
                    // Malformed frame — skip instead of tearing the session.
                    // Observed live: SpotAcPower reply occasionally has bad
                    // trailer byte. Log hex for later analysis.
                    let hex: String = bytes
                        .iter()
                        .map(|b| format!("{:02x}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    warn!(
                        attempt,
                        phase,
                        len = bytes.len(),
                        error = %e,
                        bytes = %hex,
                        "skipping malformed frame in recv_until_l1_ctrl"
                    );
                    continue;
                }
            };
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

    /// Receive frames until we see an L2-wrapped frame (ctrl=0x0001) whose
    /// L2 header `pkt_id` matches `want_pkt_id`. SBFspot's getPacket loops
    /// on pkt_id mismatch so replies to earlier requests (that might still
    /// be in the receive buffer) get discarded instead of being consumed
    /// as the reply to the current request.
    ///
    /// Returns (raw frame bytes, parsed L2 header, cmd body).
    async fn recv_l2_with_pkt_id(
        &mut self,
        want_pkt_id: u16,
        phase: &'static str,
    ) -> Result<(Vec<u8>, crate::session::L2Reply)> {
        for attempt in 0..16 {
            let bytes = self.recv_until_l1_ctrl(0x0001, phase).await?;
            let frame = match Frame::parse(&bytes) {
                Ok(f) => f,
                Err(e) => {
                    // Malformed frame (length mismatch, checksum, stuffing):
                    // log the hex + skip. One bad frame shouldn't kill the
                    // whole session — the inverter occasionally sends oddly
                    // framed diagnostic data that we don't care about.
                    let hex: String = bytes
                        .iter()
                        .map(|b| format!("{:02x}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    warn!(
                        attempt,
                        phase,
                        len = bytes.len(),
                        error = %e,
                        bytes = %hex,
                        "skipping malformed L2 frame"
                    );
                    continue;
                }
            };
            let (hdr, data) = match decode_l2(&frame.payload) {
                Some(x) => x,
                None => {
                    debug!(attempt, phase, "skipping non-L2 ctrl=0x0001 frame");
                    continue;
                }
            };
            if hdr.pkt_id == want_pkt_id {
                return Ok((
                    bytes,
                    L2Reply {
                        pkt_id: hdr.pkt_id,
                        error_code: hdr.error_code,
                        app_susy_id: hdr.app_susy_id,
                        app_serial: hdr.app_serial,
                        body: data.to_vec(),
                    },
                ));
            }
            debug!(
                attempt,
                phase,
                want_pkt = want_pkt_id,
                got_pkt = hdr.pkt_id,
                got_susy = hdr.app_susy_id,
                got_serial = hdr.app_serial,
                "skipping L2 reply with non-matching pkt_id"
            );
        }
        Err(SessionError::Silent { phase })
    }

    /// Loop through incoming L2 ctrl=0x0001 replies until we find one with
    /// matching pkt_id AND L1 source BT address matching `target_bt`. Used
    /// for init where we specifically need the reply from OUR inverter,
    /// not a MIS peer device.
    async fn recv_init_from_target(
        &mut self,
        want_pkt_id: u16,
        target_bt: [u8; 6],
    ) -> Result<L2Reply> {
        for attempt in 0..16 {
            let bytes = self.recv_until_l1_ctrl(0x0001, "init").await?;
            let frame = match Frame::parse(&bytes) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let (hdr, data) = match decode_l2(&frame.payload) {
                Some(x) => x,
                None => continue,
            };
            if hdr.pkt_id != want_pkt_id {
                debug!(
                    attempt,
                    want_pkt = want_pkt_id,
                    got_pkt = hdr.pkt_id,
                    "init skip — non-matching pkt_id"
                );
                continue;
            }
            if frame.local_bt != target_bt {
                debug!(
                    attempt,
                    target = ?target_bt,
                    got_src = ?frame.local_bt,
                    got_susy = hdr.app_susy_id,
                    got_serial = hdr.app_serial,
                    "init skip — reply from wrong BT source"
                );
                continue;
            }
            return Ok(L2Reply {
                pkt_id: hdr.pkt_id,
                error_code: hdr.error_code,
                app_susy_id: hdr.app_susy_id,
                app_serial: hdr.app_serial,
                body: data.to_vec(),
            });
        }
        Err(SessionError::Silent { phase: "init" })
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

        // ── Step 0: send "ver\r\n" discovery packet.
        //
        // SBFspot's MIS path sends this first (SBFspot.cpp:432). The
        // single-inverter path doesn't, but our user's network has 2
        // inverters sharing the same NetID so we need MIS-style init.
        // Byte-exact frame validated by `frame_builder_matches_captured_
        // discovery_packet` against 0000-send.hex.
        let ver_wire = FrameBuilder::new_with_kind(
            FrameKind::L1Only,
            [0u8; 6],               // src = zeros (matches capture)
            [1, 0, 0, 0, 0, 0],     // dst = "1.0.0" version
            0x0201,
        )
        .extend(b"ver\r\n")
        .build();
        self.transport.send_frame(&ver_wire).await?;
        debug!("ver\\r\\n sent (MIS discovery)");

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

        // Loop init replies: only accept the one where the L1 src BT matches
        // our target inverter. SMA MIS networks have multiple devices
        // (repeater + other inverters); each replies with its OWN
        // SUSyID/Serial in the L2 header. If we take the first reply blindly
        // we end up addressing queries to a relay device that then returns
        // retcode=0xFFFF ("I don't have that LRI") for everything.
        let init_reply = self.recv_init_from_target(pkt_id, inverter_bt).await?;
        self.inverter_susy_id = init_reply.app_susy_id;
        self.inverter_serial = init_reply.app_serial;
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

        // Give the inverter time to process the logoff before hitting it with
        // logon. Without this delay repeat sessions (reconnect after a parse
        // error etc.) get retcode 0x0001 from the inverter — interpreted as
        // "session still active". 300ms is enough per live observation.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

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

        // ── Step 7: recv logon reply.
        //
        // In a multi-inverter BT network (NetID>1), the logon broadcast
        // reaches every device. Each replies. We accept the FIRST reply with
        // matching pkt_id AND retcode=0 — SBFspot does the same via its
        // `validPcktID` loop. A single inverter returning 0x0001 doesn't
        // necessarily mean logon failed globally; it can mean "this device
        // isn't responding to that pkt_id's intended target".
        let logon_reply = {
            let mut last_reject: Option<L2Reply> = None;
            let mut logged_in: Option<L2Reply> = None;
            for _ in 0..16 {
                let bytes = match self.recv_until_l1_ctrl(0x0001, "logon").await {
                    Ok(b) => b,
                    Err(_) => break,
                };
                let frame = match Frame::parse(&bytes) {
                    Ok(f) => f,
                    Err(_) => continue,
                };
                let (hdr, data) = match decode_l2(&frame.payload) {
                    Some(x) => x,
                    None => continue,
                };
                if hdr.pkt_id != pkt_id {
                    continue;
                }
                let reply = L2Reply {
                    pkt_id: hdr.pkt_id,
                    error_code: hdr.error_code,
                    app_susy_id: hdr.app_susy_id,
                    app_serial: hdr.app_serial,
                    body: data.to_vec(),
                };
                // 0x0001 has been observed as "session already active" —
                // not a hard rejection. If both inverters return 0x0001 we
                // proceed with queries; if queries succeed, the session is
                // effectively logged-in. 0x0100 is still hard-fail (bad
                // password). Anything else: continue waiting.
                if reply.error_code == 0 {
                    logged_in = Some(reply);
                    break;
                }
                debug!(
                    code = format!("0x{:04x}", reply.error_code),
                    susy = reply.app_susy_id,
                    serial = reply.app_serial,
                    "logon reply rejected by this device — waiting for another"
                );
                last_reject = Some(reply);
            }
            match logged_in {
                Some(r) => r,
                None => {
                    if let Some(rej) = last_reject.as_ref() {
                        if rej.error_code == 0x0100 {
                            tracing::warn!(
                                susy = rej.app_susy_id,
                                serial = rej.app_serial,
                                "logon rejected: invalid password"
                            );
                            return Err(SessionError::LogonFailed { code: 0x0100 });
                        }
                        tracing::warn!(
                            code = format!("0x{:04x}", rej.error_code),
                            susy = rej.app_susy_id,
                            serial = rej.app_serial,
                            "no inverter accepted logon"
                        );
                        return Err(SessionError::LogonFailed { code: rej.error_code });
                    }
                    return Err(SessionError::Silent { phase: "logon" });
                }
            }
        };

        // If the accepting device isn't the one we identified in init, that's
        // OK in a MIS network — the logon broadcast was accepted by a peer.
        // Queries still target self.inverter_{susy,serial} set from init.
        if logon_reply.app_susy_id != self.inverter_susy_id
            || logon_reply.app_serial != self.inverter_serial
        {
            debug!(
                init_susy = self.inverter_susy_id,
                init_serial = self.inverter_serial,
                logon_susy = logon_reply.app_susy_id,
                logon_serial = logon_reply.app_serial,
                "logon accepted by peer (MIS network)"
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

        // Match reply on sent pkt_id (SBFspot's approach). Skips stray
        // replies from previous requests that may still be in the buffer.
        let (_bytes, reply) = self.recv_l2_with_pkt_id(pkt_id, "query").await?;
        let hex_body: String = reply
            .body
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" ");
        debug!(
            ?kind,
            reply_susy = reply.app_susy_id,
            reply_serial = reply.app_serial,
            reply_pkt_id = reply.pkt_id,
            reply_retcode = reply.error_code,
            body_len = reply.body.len(),
            body = %hex_body,
            "query reply"
        );
        Ok(reply.body)
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
#[allow(dead_code)]
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
