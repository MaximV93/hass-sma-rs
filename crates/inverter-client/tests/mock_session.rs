//! End-to-end session test driven by MockTransport.
//!
//! Validates the full handshake → logon → query flow without real BT. Replies
//! are fabricated to match the byte-layout the protocol crate produces on the
//! reverse direction.

use bluez_transport::MockTransport;
use inverter_client::{
    session::{Session, SessionConfig, SessionState},
    values::parse_spot_ac_total_power,
};
use sma_bt_protocol::{
    auth::UserGroup,
    commands::QueryKind,
    frame::{FrameBuilder, FrameKind},
    packet::{encode_l2, L2Header},
    APP_SUSY_ID,
};

/// Build a fake "hello" frame the inverter would send right after connect.
/// Real hello is L1-only (no L2 signature) with ctrl=0x0002.
/// Byte 19 of the wire frame (== payload[1]) = firmware version (≥ 4 required).
/// Byte 22 (== payload[4]) = NetID.
fn fake_hello(local: [u8; 6], dest: [u8; 6]) -> Vec<u8> {
    let mut b = FrameBuilder::new_with_kind(FrameKind::L1Only, local, dest, 0x0002);
    // payload layout mirroring real hello: 0x00700400 | NetID | 0 | 1
    // Bytes at frame offset: [18]=0x00 [19]=0x04 (fw) [20]=0x70 [21]=0x00 [22]=0x02 (netid)
    b.extend(&[
        0x00, 0x04, 0x70, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    ]);
    b.build()
}

/// Fake topology broadcast (ctrl=0x0005, L1-only). Real one carries a list
/// of BT addresses but the session only cares about the ctrl match.
fn fake_topology(local: [u8; 6], dest: [u8; 6]) -> Vec<u8> {
    let mut b = FrameBuilder::new_with_kind(FrameKind::L1Only, local, dest, 0x0005);
    b.extend(&[0; 32]);
    b.build()
}

/// Fake init-reply. Session's recv_l2_with_pkt_id matches on the sent
/// pkt_id, so the reply must echo it.
fn fake_init_reply(local: [u8; 6], dest: [u8; 6], pkt_id: u16) -> Vec<u8> {
    let hdr = L2Header {
        longwords: 0x09,
        ctrl: 0xA0,
        dst_susy_id: APP_SUSY_ID,
        dst_serial: 900_123_456,
        ctrl2: 0x0000,
        app_susy_id: 101,
        app_serial: 2_120_121_246,
        error_code: 0,
        pkt_id,
    };
    let body = [0u8; 16];
    let l2 = encode_l2(&hdr, &body);
    let mut b = FrameBuilder::new(local, dest, 0x0001);
    b.extend(&l2);
    b.build()
}

/// Fake successful logon reply echoing sent pkt_id.
fn fake_logon_reply(local: [u8; 6], dest: [u8; 6], pkt_id: u16) -> Vec<u8> {
    let hdr = L2Header {
        longwords: 0x0E,
        ctrl: 0xA0,
        dst_susy_id: APP_SUSY_ID,
        dst_serial: 900_123_456,
        ctrl2: 0x0100,
        app_susy_id: 101,
        app_serial: 2_120_121_246,
        error_code: 0,
        pkt_id,
    };
    let body = [0u8; 12];
    let l2 = encode_l2(&hdr, &body);
    let mut b = FrameBuilder::new(local, dest, 0x0001);
    b.extend(&l2);
    b.build()
}

/// Fake query reply with one SpotPacTotal record, echoing sent pkt_id.
fn fake_query_reply(local: [u8; 6], dest: [u8; 6], pkt_id: u16) -> Vec<u8> {
    let hdr = L2Header {
        longwords: 0x09,
        ctrl: 0xA0,
        dst_susy_id: APP_SUSY_ID,
        dst_serial: 900_123_456,
        ctrl2: 0x0000,
        app_susy_id: 101,
        app_serial: 2_120_121_246,
        error_code: 0,
        pkt_id,
    };

    // Real reply shape: 12-byte opcode/LRI echo prefix, then 28-byte record.
    // Record code = 0x00_26_3F_01 (class 0x01 in low byte, LRI 0x00263F00 in
    // middle). Value at [16..20] — NOT [8..12] (those are min/max slots).
    let mut cmd_body = Vec::with_capacity(40);
    cmd_body.extend_from_slice(&[0u8; 12]); // prefix
    let mut rec = [0u8; 28];
    rec[0..4].copy_from_slice(&0x0026_3F01u32.to_le_bytes());
    rec[4..8].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    rec[8..12].copy_from_slice(&i32::MIN.to_le_bytes()); // NaN slot
    rec[12..16].copy_from_slice(&i32::MIN.to_le_bytes());
    rec[16..20].copy_from_slice(&1234i32.to_le_bytes()); // the real value
    rec[20..24].copy_from_slice(&i32::MIN.to_le_bytes());
    rec[24..28].copy_from_slice(&i32::MIN.to_le_bytes());
    cmd_body.extend_from_slice(&rec);

    let l2 = encode_l2(&hdr, &cmd_body);
    let mut b = FrameBuilder::new(local, dest, 0x0001);
    b.extend(&l2);
    b.build()
}

#[tokio::test]
async fn full_session_happy_path() {
    let local: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
    let inverter: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

    let mock = MockTransport::new();
    // Inverter's pre-scripted replies. Order mirrors the new handshake:
    //   recv hello → send echo → recv topology → send init → recv init_reply
    //   → send logoff → send logon → recv logon_reply → send query → recv query_reply
    // Session pkt_id sequence: 1=init, 2=logoff, 3=logon, 4=first query.
    mock.queue_replies(vec![
        fake_hello(inverter, local),
        fake_topology(inverter, local),
        fake_init_reply(inverter, local, 1),
        fake_logon_reply(inverter, local, 3),
        fake_query_reply(inverter, local, 4),
    ]);

    let cfg = SessionConfig {
        inverter_bt: inverter,
        local_bt: local,
        password: "0000".to_string(),
        user_group: UserGroup::User,
        timeout_ms: 1000,
        mis_enabled: false,
    };
    let mut session = Session::new(mock, cfg);
    assert_eq!(session.state(), SessionState::Disconnected);

    // Handshake + logon
    session.handshake_and_logon().await.expect("handshake OK");
    assert_eq!(session.state(), SessionState::LoggedIn);
    assert_eq!(session.inverter_susy_id, 101);
    assert_eq!(session.inverter_serial, 2_120_121_246);

    // Query
    let raw = session
        .query(QueryKind::SpotAcTotalPower)
        .await
        .expect("query OK");
    let readings = parse_spot_ac_total_power(&raw);
    assert_eq!(readings.pac_total_w, Some(1234));
    assert_eq!(readings.timestamp, Some(0x1234_5678));

    session.close().await.unwrap();
    assert_eq!(session.state(), SessionState::Disconnected);
}

#[tokio::test]
async fn logon_failure_is_typed() {
    use bluez_transport::MockTransport;
    use inverter_client::session::SessionError;
    use sma_bt_protocol::{
        frame::FrameBuilder,
        packet::{encode_l2, L2Header},
        APP_SUSY_ID,
    };

    let local: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
    let inverter: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

    let mock = MockTransport::new();
    mock.queue_reply(fake_hello(inverter, local));
    mock.queue_reply(fake_topology(inverter, local));
    mock.queue_reply(fake_init_reply(inverter, local, 1));

    // Logon reply with retcode 0x0100 — pkt_id must match sent logon (=3).
    let hdr = L2Header {
        longwords: 0x0E,
        ctrl: 0xA0,
        dst_susy_id: APP_SUSY_ID,
        dst_serial: 900_123_456,
        ctrl2: 0x0100,
        app_susy_id: 101,
        app_serial: 2_120_121_246,
        error_code: 0x0100,
        pkt_id: 3,
    };
    let body = [0u8; 12];
    let l2 = encode_l2(&hdr, &body);
    let mut b = FrameBuilder::new(inverter, local, 0x0001);
    b.extend(&l2);
    mock.queue_reply(b.build());

    let cfg = SessionConfig {
        inverter_bt: inverter,
        local_bt: local,
        password: "wrong".to_string(),
        user_group: UserGroup::User,
        timeout_ms: 1000,
        mis_enabled: false,
    };
    let mut session = Session::new(mock, cfg);
    let err = session.handshake_and_logon().await.unwrap_err();
    assert!(matches!(err, SessionError::LogonFailed { code: 0x0100 }));
}

/// After a successful logon, graceful_close must send a LOGOFF frame
/// BEFORE closing the transport. Regression: the parallel-run yield
/// path used to just drop the socket, leaving the inverter holding a
/// zombie session for ~15 min and rejecting every reconnect with
/// EHOSTDOWN until its own timeout.
#[tokio::test]
async fn graceful_close_emits_logoff() {
    use sma_bt_protocol::BT_L2_SIGNATURE;

    let local: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
    let inverter: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

    let mock = MockTransport::new();
    mock.queue_replies(vec![
        fake_hello(inverter, local),
        fake_topology(inverter, local),
        fake_init_reply(inverter, local, 1),
        fake_logon_reply(inverter, local, 3),
    ]);

    let cfg = SessionConfig {
        inverter_bt: inverter,
        local_bt: local,
        password: "0000".to_string(),
        user_group: UserGroup::User,
        timeout_ms: 1000,
        mis_enabled: false,
    };
    let mut session = Session::new(mock.clone(), cfg);
    session.handshake_and_logon().await.expect("handshake OK");
    assert_eq!(session.state(), SessionState::LoggedIn);

    let pre_close_count = mock.sent_frames().len();
    session.graceful_close().await.expect("graceful close OK");
    let post_close_count = mock.sent_frames().len();

    assert_eq!(
        post_close_count,
        pre_close_count + 1,
        "graceful_close must emit exactly one extra frame (the LOGOFF)"
    );
    assert_eq!(session.state(), SessionState::Disconnected);

    // Inspect the LOGOFF frame: L2-wrapped, contains the L2 signature,
    // destination is broadcast [0xFF;6] (mirrors handshake logoff shape).
    let logoff = mock.sent_frames().last().cloned().expect("frame");
    let sig = BT_L2_SIGNATURE.to_le_bytes();
    assert!(
        logoff.windows(sig.len()).any(|w| w == sig),
        "logoff frame must carry the L2 signature"
    );
    // L1 header: [0x7E, len_lo, len_hi, cks, src(6), dst(6), ctrl_lo, ctrl_hi]
    // dst at bytes 10..16.
    assert_eq!(
        &logoff[10..16],
        &[0xFF; 6],
        "logoff must go to broadcast dst"
    );
}

/// MIS multi-device polling: one session, two inverter serials. Each
/// `query_for_device(susy, serial, kind)` call must route the target
/// serial into the outbound L2 header and the reply must carry that
/// same serial in the `app_serial` field.
#[tokio::test]
async fn mis_multi_device_routes_each_serial() {
    let local: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
    let inverter: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

    let mock = MockTransport::new();
    // Fake device A (zolder-ish) + device B (garage-ish) replies to
    // two consecutive queries. pkt_id sequence after handshake is
    // 1=init 2=logoff 3=logon then 4=first query 5=second query.
    let reply_for = |pkt_id: u16, src_serial: u32| -> Vec<u8> {
        let hdr = L2Header {
            longwords: 0x09,
            ctrl: 0xA0,
            dst_susy_id: APP_SUSY_ID,
            dst_serial: 900_123_456,
            ctrl2: 0x0000,
            app_susy_id: 131,
            app_serial: src_serial,
            error_code: 0,
            pkt_id,
        };
        let mut cmd_body = Vec::with_capacity(40);
        cmd_body.extend_from_slice(&[0u8; 12]);
        let mut rec = [0u8; 28];
        rec[0..4].copy_from_slice(&0x0026_3F01u32.to_le_bytes());
        rec[4..8].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        rec[8..12].copy_from_slice(&i32::MIN.to_le_bytes());
        rec[12..16].copy_from_slice(&i32::MIN.to_le_bytes());
        rec[16..20].copy_from_slice(&1234i32.to_le_bytes());
        rec[20..24].copy_from_slice(&i32::MIN.to_le_bytes());
        rec[24..28].copy_from_slice(&i32::MIN.to_le_bytes());
        cmd_body.extend_from_slice(&rec);
        let l2 = encode_l2(&hdr, &cmd_body);
        let mut b = FrameBuilder::new(inverter, local, 0x0001);
        b.extend(&l2);
        b.build()
    };

    mock.queue_replies(vec![
        fake_hello(inverter, local),
        fake_topology(inverter, local),
        fake_init_reply(inverter, local, 1),
        fake_logon_reply(inverter, local, 3),
        // query 1 → device A (zolder, serial 2120121246) reply at pkt 4
        reply_for(4, 2_120_121_246),
        // query 2 → device B (garage, serial 2120121383) reply at pkt 5
        reply_for(5, 2_120_121_383),
    ]);

    let cfg = SessionConfig {
        inverter_bt: inverter,
        local_bt: local,
        password: "0000".to_string(),
        user_group: UserGroup::User,
        timeout_ms: 1000,
        mis_enabled: true,
    };
    let mut session = Session::new(mock.clone(), cfg);
    session.handshake_and_logon().await.expect("handshake");

    // Round-robin per tick: query each device once.
    let body_a = session
        .query_for_device(131, 2_120_121_246, QueryKind::SpotAcTotalPower)
        .await
        .expect("query A");
    let body_b = session
        .query_for_device(131, 2_120_121_383, QueryKind::SpotAcTotalPower)
        .await
        .expect("query B");

    // Both replies parse cleanly to the same synthetic value.
    let a = parse_spot_ac_total_power(&body_a);
    let b = parse_spot_ac_total_power(&body_b);
    assert_eq!(a.pac_total_w, Some(1234));
    assert_eq!(b.pac_total_w, Some(1234));

    // Verify the outbound frames carry the correct dst_serial in the
    // L2 header for each target. Use Frame::parse + decode_l2 rather
    // than raw byte search — the serial 0x7E5E5A5E contains 0x7E
    // which byte-stuffing escapes to `0x7D 0x5E`, so a naive search
    // misses it. Parse re-assembles the original bytes.
    let sent = mock.sent_frames();
    assert!(sent.len() >= 6, "expected at least 6 outbound frames");

    let find_dst_serial = |f: &[u8]| -> Option<u32> {
        use sma_bt_protocol::{decode_l2, Frame, FrameKind};
        let frame = Frame::parse(f).ok()?;
        if !matches!(frame.kind, FrameKind::L2Wrapped) {
            return None;
        }
        let (hdr, _) = decode_l2(&frame.payload)?;
        Some(hdr.dst_serial)
    };

    let serial_a_hit = sent.iter().filter_map(|f| find_dst_serial(f))
        .any(|s| s == 2_120_121_246);
    let serial_b_hit = sent.iter().filter_map(|f| find_dst_serial(f))
        .any(|s| s == 2_120_121_383);
    assert!(serial_a_hit, "at least one outbound frame must address device A's serial (2120121246)");
    assert!(serial_b_hit, "at least one outbound frame must address device B's serial (2120121383)");
}

/// Event log session method: sends a query, receives a single-fragment
/// reply, aggregates, and returns the body. Multi-fragment behavior is
/// hard to mock-test without knowing the real fragment-id protocol bit;
/// the test ensures at minimum the wire path compiles and returns data.
#[tokio::test]
async fn event_log_query_single_fragment_roundtrip() {
    let local: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
    let inverter: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

    let mock = MockTransport::new();
    // Reply carrying one 24-byte event record — pkt_id after
    // handshake is 4 for the first event-log query.
    let reply = {
        let hdr = L2Header {
            longwords: 0x09,
            ctrl: 0xE0,
            dst_susy_id: APP_SUSY_ID,
            dst_serial: 900_123_456,
            ctrl2: 0x0000,
            app_susy_id: 131,
            app_serial: 2_120_121_246,
            error_code: 0,
            pkt_id: 4,
        };
        // 12-byte opcode-echo prefix + one 24-byte event record.
        let mut body = vec![0u8; 12 + 24];
        let off = 12;
        body[off..off + 4].copy_from_slice(&42u32.to_le_bytes());
        body[off + 4..off + 8].copy_from_slice(&1_700_000_000u32.to_le_bytes());
        body[off + 8..off + 12].copy_from_slice(&0x0000_0133u32.to_le_bytes()); // tag 307
        // Final fragment: body size < first-fragment baseline causes
        // aggregator to stop after this one.
        let l2 = encode_l2(&hdr, &body);
        let mut b = FrameBuilder::new(inverter, local, 0x0001);
        b.extend(&l2);
        b.build()
    };
    // Second reply (body size 12 = header only, no records) signals
    // "end of stream" per aggregator's body-size heuristic.
    let sentinel = {
        let hdr = L2Header {
            longwords: 0x09,
            ctrl: 0xE0,
            dst_susy_id: APP_SUSY_ID,
            dst_serial: 900_123_456,
            ctrl2: 0x0000,
            app_susy_id: 131,
            app_serial: 2_120_121_246,
            error_code: 0,
            pkt_id: 4,
        };
        let body = vec![0u8; 12];
        let l2 = encode_l2(&hdr, &body);
        let mut b = FrameBuilder::new(inverter, local, 0x0001);
        b.extend(&l2);
        b.build()
    };
    mock.queue_replies(vec![
        fake_hello(inverter, local),
        fake_topology(inverter, local),
        fake_init_reply(inverter, local, 1),
        fake_logon_reply(inverter, local, 3),
        reply,
        sentinel,
    ]);

    let cfg = SessionConfig {
        inverter_bt: inverter,
        local_bt: local,
        password: "0000".to_string(),
        user_group: UserGroup::User,
        timeout_ms: 1000,
        mis_enabled: false,
    };
    let mut session = Session::new(mock, cfg);
    session.handshake_and_logon().await.expect("handshake");

    let aggregated = session
        .query_event_log_for_device(131, 2_120_121_246, 1_700_000_000, 1_700_086_400)
        .await
        .expect("event log query");

    // Aggregated body should contain our record — inverter_client's
    // `parse_event_log_records` turns it into a typed event.
    use inverter_client::values::parse_event_log_records;
    let recs = parse_event_log_records(&aggregated);
    assert!(
        !recs.is_empty(),
        "aggregated body should parse to ≥ 1 event record"
    );
    // Our synthetic record used tag 0x133 = 307 "Normal operation".
    assert!(recs.iter().any(|r| r.tag() == 307));
}

/// graceful_close on a non-logged-in session must not crash and must
/// not attempt a send. Safe to call on error paths where logon never
/// completed.
#[tokio::test]
async fn graceful_close_safe_when_not_logged_in() {
    let local: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
    let inverter: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

    let mock = MockTransport::new();
    let cfg = SessionConfig {
        inverter_bt: inverter,
        local_bt: local,
        password: "0000".to_string(),
        user_group: UserGroup::User,
        timeout_ms: 1000,
        mis_enabled: false,
    };
    let mut session = Session::new(mock.clone(), cfg);
    assert_eq!(session.state(), SessionState::Disconnected);
    session.graceful_close().await.expect("safe close");
    assert!(
        mock.sent_frames().is_empty(),
        "no logoff must be sent if session never reached LoggedIn"
    );
}
