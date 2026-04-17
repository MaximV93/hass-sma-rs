//! End-to-end session test driven by MockTransport.
//!
//! Validates the full handshake → logon → query flow without real BT. Replies
//! are fabricated to match the byte-layout the protocol crate produces on the
//! reverse direction.

use bluez_transport::{MockTransport, Transport};
use inverter_client::{
    session::{Session, SessionConfig, SessionState},
    values::parse_spot_ac_total_power,
};
use sma_bt_protocol::{
    auth::UserGroup,
    commands::QueryKind,
    frame::FrameBuilder,
    packet::{encode_l2, L2Header},
    APP_SUSY_ID,
};

/// Build a fake "hello" frame the inverter would send right after connect.
fn fake_hello(local: [u8; 6], dest: [u8; 6]) -> Vec<u8> {
    // Minimal L2 payload so the frame builder is happy.
    let mut b = FrameBuilder::new(local, dest, 0x0002);
    b.extend(&[0xFF, 0x03, 0x60, 0x65, 0, 0, 0, 0]);
    b.build()
}

/// Build a fake `ver` reply. Byte 19 is the firmware-protocol version (>= 4
/// means "modern firmware"); SBFspot rejects anything below 4.
fn fake_ver_reply(local: [u8; 6], dest: [u8; 6]) -> Vec<u8> {
    let mut payload = vec![0u8; 32];
    // Pad so that byte 19 (relative to frame start with 18-byte L1 header +
    // minimal payload) ends up as 0x04 or higher. Easier: put `0x04` at
    // position 1 within payload -> position 19 of total frame.
    payload[1] = 0x04;
    let mut b = FrameBuilder::new(local, dest, 0x0002);
    b.extend(&payload);
    b.build()
}

/// Build a fake successful logon reply. L2 body starts with 2-byte retcode (0 = OK).
fn fake_logon_reply(local: [u8; 6], dest: [u8; 6]) -> Vec<u8> {
    let hdr = L2Header {
        longwords: 0x0E,
        ctrl: 0xA0,
        dst_susy_id: APP_SUSY_ID,
        dst_serial: 900_123_456,
        ctrl2: 0x0100,
        app_susy_id: 101, // pretend inverter SUSyID
        app_serial: 2_120_121_246, // pretend inverter serial
        pkt_id: 0x0001,
    };
    // Body: retcode 0x0000 + filler
    let body = [0u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let l2 = encode_l2(&hdr, &body);
    let mut b = FrameBuilder::new(local, dest, 0x0001);
    b.extend(&l2);
    b.build()
}

/// Build a fake query reply with one SpotPacTotal record encoding 1234 W.
fn fake_query_reply(local: [u8; 6], dest: [u8; 6]) -> Vec<u8> {
    let hdr = L2Header {
        longwords: 0x09,
        ctrl: 0xA0,
        dst_susy_id: APP_SUSY_ID,
        dst_serial: 900_123_456,
        ctrl2: 0x0000,
        app_susy_id: 101,
        app_serial: 2_120_121_246,
        pkt_id: 0x0002,
    };

    // One 28-byte record: LRI 0x0026_3F00 at [0..4] → PAC total, value 1234.
    let mut rec = [0u8; 28];
    rec[0..4].copy_from_slice(&0x0026_3F00u32.to_le_bytes());
    rec[4..8].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    rec[8..12].copy_from_slice(&1234i32.to_le_bytes());

    let l2 = encode_l2(&hdr, &rec);
    let mut b = FrameBuilder::new(local, dest, 0x0001);
    b.extend(&l2);
    b.build()
}

#[tokio::test]
async fn full_session_happy_path() {
    let local: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
    let inverter: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

    let mock = MockTransport::new();
    // Inverter's pre-scripted replies (order = order the session reads them).
    mock.queue_replies(vec![
        fake_hello(inverter, local),
        fake_ver_reply(inverter, local),
        fake_logon_reply(inverter, local),
        fake_query_reply(inverter, local),
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
    use bluez_transport::{MockTransport, Transport};
    use inverter_client::session::SessionError;
    use sma_bt_protocol::{frame::FrameBuilder, packet::{encode_l2, L2Header}, APP_SUSY_ID};

    let local: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
    let inverter: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

    let mock = MockTransport::new();
    mock.queue_reply(fake_hello(inverter, local));
    mock.queue_reply(fake_ver_reply(inverter, local));

    // Logon reply with retcode 0x0100 (invalid password)
    let hdr = L2Header {
        longwords: 0x0E,
        ctrl: 0xA0,
        dst_susy_id: APP_SUSY_ID,
        dst_serial: 900_123_456,
        ctrl2: 0x0100,
        app_susy_id: 101,
        app_serial: 2_120_121_246,
        pkt_id: 0x0001,
    };
    let body = [0x00, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]; // retcode LE = 0x0100
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
