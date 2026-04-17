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
    b.extend(&[0x00, 0x04, 0x70, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);
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
