//! Integration test: load every `.hex` fixture from `tests/fixtures/captured/`
//! and assert that `Frame::parse` accepts it without error.
//!
//! This test is *additive* — if no fixtures exist yet it's a no-op. When
//! real captures land (via `scripts/parse-sbfspot-hexdump.py`) the test
//! automatically gains coverage.

use sma_bt_protocol::{parse_l2_only_blob, Frame, FrameBuilder, FrameKind};
use std::fs;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    // Relative to crate root; `tests/` dir at repo root sits beside crate.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent() // crates/
        .unwrap()
        .parent() // repo/
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("captured")
}

fn hex_line_to_bytes(line: &str) -> Vec<u8> {
    line.split_whitespace()
        .map(|t| u8::from_str_radix(t, 16).expect("hex byte"))
        .collect()
}

/// Frames observed in SBFspot's pcktBuf dumps come in three shapes:
///
/// 1. Full L1-header frames (what gets sent/received on the wire). Parsable
///    via `Frame::parse`.
/// 2. "L2-only" blobs that SBFspot emits for *received* frames after it has
///    already stripped the L1 header. These start with `7e ff 03 60 65` (L2
///    signature immediately after the leading delimiter).
/// 3. Truncated/mid-stream dumps: SBFspot's HexDump stops at a protocol-
///    specific boundary that doesn't always align with a full frame.
///
/// We classify each fixture into one of those three buckets. `Frame::parse`
/// must succeed on bucket 1. Bucket 2 is structurally valid but a different
/// shape (tracked but not fatal). Bucket 3 is an SBFspot quirk — tracked.
#[test]
fn all_captured_frames_parse() {
    let dir = fixtures_dir();
    if !dir.is_dir() {
        eprintln!("no captured/ fixtures yet at {}", dir.display());
        return;
    }

    let mut l1_ok = 0;
    let mut l2_only = 0;
    let mut truncated = 0;
    let mut l1_failed = Vec::new();

    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("hex"))
        .collect();
    paths.sort();

    for path in &paths {
        let content = fs::read_to_string(path).unwrap();
        let line = match content.lines().next() {
            Some(l) => l.trim(),
            None => continue,
        };
        if line.is_empty() {
            continue;
        }
        let raw = hex_line_to_bytes(line);
        if raw.len() < 5 {
            truncated += 1;
            continue;
        }
        // Bucket 2: L2-only blob starts with `7e ff 03 60 65`. Validate via
        // parse_l2_only_blob — FCS must match, stuffing must unpack cleanly.
        if raw[0] == 0x7E && raw.get(1..5) == Some(&[0xFF, 0x03, 0x60, 0x65]) {
            match parse_l2_only_blob(&raw) {
                Ok(_) => l2_only += 1,
                Err(e) => l1_failed
                    .push(format!("{}: L2 blob: {}", path.display(), e)),
            }
            continue;
        }
        match Frame::parse(&raw) {
            Ok(_) => l1_ok += 1,
            Err(e) => l1_failed.push(format!("{}: {}", path.display(), e)),
        }
    }

    eprintln!(
        "captured: L1-valid={}, L2-only-blob={}, truncated={}, L1-failed={}",
        l1_ok,
        l2_only,
        truncated,
        l1_failed.len()
    );
    if !l1_failed.is_empty() {
        panic!(
            "{} L1-shaped fixtures failed to parse:\n{}",
            l1_failed.len(),
            l1_failed.join("\n")
        );
    }
    // Make sure we actually saw something.
    assert!(l1_ok > 0 || l2_only > 0, "no fixture frames classified");
}

/// Contract test: verify our FrameBuilder produces the exact same bytes
/// SBFspot emits for the canonical "ver\r\n" discovery packet (first
/// outbound frame in any handshake).
///
/// Expected byte layout (from real inverter capture, fixture 0000-send.hex):
///
///     7e 17 00 69  00 00 00 00 00 00  01 00 00 00 00 00  01 02
///     |  |  |  |   └ local_bt (zeros)┘└── dest_bt ─────┘└ ctrl ┘
///     |  |  └──── hdr_cks = 0x7E ^ 0x17 ^ 0x00
///     |  └─── total frame length (little-endian), 0x0017 = 23
///     └── frame start delimiter
///
///     76 65 72 0d 0a       = "ver\r\n"
///
/// No FCS, no trailing 0x7E — that's the L1-only signature.
#[test]
fn frame_builder_matches_captured_discovery_packet() {
    let local_bt: [u8; 6] = [0; 6];
    let dest_bt: [u8; 6] = [1, 0, 0, 0, 0, 0];
    let mut b = FrameBuilder::new_with_kind(FrameKind::L1Only, local_bt, dest_bt, 0x0201);
    b.extend(b"ver\r\n");
    let wire = b.build();

    let fixture = hex_line_to_bytes(
        "7e 17 00 69 00 00 00 00 00 00 01 00 00 00 00 00 01 02 76 65 72 0d 0a",
    );
    assert_eq!(wire, fixture, "built frame must match captured bytes exactly");
}

/// Contract test: an L2-wrapped frame the FrameBuilder emits must be
/// roundtrip-able by Frame::parse back to the original payload. Also asserts
/// the on-wire shape includes the 0x7E separator at offset 18 — critical for
/// interop with real inverters.
///
/// We don't assert byte-exactness against a captured logon here because the
/// logon body embeds password, session id, and epoch — values that vary each
/// run. The per-payload wire format (separator + stuffing + FCS + trailer) is
/// what this test locks in.
#[test]
fn l2_frame_has_separator_and_roundtrips() {
    let local: [u8; 6] = [0x74, 0x37, 0x5a, 0x1a, 0x42, 0x04];
    let dest: [u8; 6] = [0xFF; 6];
    let mut b = FrameBuilder::new(local, dest, 0x0001);
    // Plausible L2 body: signature + header + body bytes (not stuffed).
    let payload = [
        0xFF, 0x03, 0x60, 0x65, 0x09, 0xA0, // sig + longwords/ctrl
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // broadcast dst
        0x00, 0x00, 0x7E, 0x00, 0x50, 0xFB, 0x2D, 0x3B, // carries a 0x7E that MUST get stuffed
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // padding
    ];
    b.extend(&payload);
    let wire = b.build();

    // Structural contract: separator byte at offset 18, trailer at end.
    assert_eq!(wire[0], 0x7E, "leading delimiter");
    assert_eq!(wire[18], 0x7E, "L2 separator at offset 18");
    assert_eq!(*wire.last().unwrap(), 0x7E, "trailing delimiter");

    // The 0x7E inside the payload at byte 14 must have been stuffed as 0x7D 0x5E
    // — search for it somewhere in the stuffed region [19..len-1].
    let stuffed_region = &wire[19..wire.len() - 1];
    let mut found_escape = false;
    for w in stuffed_region.windows(2) {
        if w == [0x7D, 0x5E] {
            found_escape = true;
            break;
        }
    }
    assert!(found_escape, "inner 0x7E must be byte-stuffed as 0x7D 0x5E");

    // Roundtrip: parse restores the original (unstuffed) payload.
    let parsed = Frame::parse(&wire).unwrap();
    assert!(matches!(parsed.kind, sma_bt_protocol::FrameKind::L2Wrapped));
    assert_eq!(parsed.local_bt, local);
    assert_eq!(parsed.dest_bt, dest);
    assert_eq!(parsed.control, 0x0001);
    assert_eq!(parsed.payload, payload);
}
