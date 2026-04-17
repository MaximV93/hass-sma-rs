//! Integration test: load every `.hex` fixture from `tests/fixtures/captured/`
//! and assert that `Frame::parse` accepts it without error.
//!
//! This test is *additive* — if no fixtures exist yet it's a no-op. When
//! real captures land (via `scripts/parse-sbfspot-hexdump.py`) the test
//! automatically gains coverage.

use sma_bt_protocol::Frame;
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

#[test]
fn all_captured_frames_parse() {
    let dir = fixtures_dir();
    if !dir.is_dir() {
        eprintln!("no captured/ fixtures yet at {}", dir.display());
        return;
    }

    let mut count = 0;
    let mut failures = Vec::new();

    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("hex") {
            continue;
        }
        let content = fs::read_to_string(&path).unwrap();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let raw = hex_line_to_bytes(line);
            match Frame::parse(&raw) {
                Ok(_) => count += 1,
                Err(e) => failures.push(format!("{}: {}", path.display(), e)),
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} fixture frames failed to parse:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
    eprintln!("parsed {} fixture frames", count);
}
