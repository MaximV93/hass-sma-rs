# Changelog

All notable changes to hass-sma-rs are tracked here. This project follows
semantic-ish versioning; pre-1.0 is rapid iteration.

## 0.1.33 — 2026-04-18

### Fixed
- **DeviceStatus decoder** scans all 4 status tag slots at record offsets
  8/12/16/20 and picks the one with the `0x01000000` "selected" bit. Was
  reading offset 8 blindly which always returned the first (unselected)
  tag. User-facing effect: Inverter Status showed "Error" even when healthy.
- **`status` MQTT key collision** — daemon's cycle-success handler and the
  DeviceStatus query handler both wrote to `status`. Cycle-success always
  won. Split into two sensors: `inverter_state` (from query) and
  `poll_status` (from daemon cycle health).

## 0.1.32 — 2026-04-18

### Added
- **Parallel-run yield window** — per-inverter `yield_every` + `yield_duration`
  options. Every Nth poll cycle the daemon drops its BT session for a
  configurable window so another SMA integration (haos-sbfspot, Sunny
  Explorer) can poll. Reconnect uses stable app_serial → no session conflict.

## 0.1.31 — 2026-04-18

### Fixed
- **parse_dc_block short records** — it had its own per-stride loop that
  missed the earlier 20-byte-record fix. `dc_current_string_1` was stuck
  on unknown because the IDC record in SpotDcVoltage replies is 20 bytes
  (vs the 28-byte default). Now accepts MIN=20..MAX=28 stride.

## 0.1.30 — 2026-04-18

### Fixed
- **20-byte short records** — some inverter replies truncate the last record
  to 20 bytes when trailing min/max/flag fields are all NaN. Parser now
  scans at MIN=20 with stride-up-to-28 fallback.

### Added
- **Comparison documentation** — `docs/COMPARISON.md` three-way feature
  matrix for stock SBFspot vs haos-sbfspot vs hass-sma-rs.

## 0.1.29 — 2026-04-18

### Fixed
- **CI cosign regression** — sigstore/cosign-installer@v4.1.1 hardcodes
  cosign v2.5.3 which the installer's own validation now rejects. Removed
  the sign/attest steps from the Builder workflow (private fork, no
  downstream signature consumers). Re-enable once upstream fixes.

## 0.1.28, 0.1.27 — 2026-04-18

### Protocol (session-layer)
- **Accept L2 replies on any L1 ctrl** — `recv_l2_with_pkt_id` used to
  filter on `recv_until_l1_ctrl(0x0001)`, silently skipping every
  `ctrl=0x0008` unsolicited push. Inverter sends SpotAcPower data as such
  a push with matching pkt_id but different L1 ctrl. Now any L2-wrapped
  frame with matching pkt_id is accepted.

## 0.1.26 — 2026-04-18

### Protocol (frame layer)
- **Parse L2 frames without trailing 0x7E** — ctrl=0x0008 push frames have
  no FCS-16 trailer, body runs to end. `Frame::parse` now branches on
  `raw.last() == 0x7E` and skips the FCS strip for push shapes.

### Fixed
- **Software version parsing** — moved from record offset 16 (where plain
  i32 values live) to offset 24, with BCD decoding for major/minor bytes
  and the "NEABRS" release-type table. 02.30.06.R now shows correctly.

## 0.1.25 — 2026-04-18

### Fixed
- **Stable app_serial across reconnects** — each `Session::new()` was
  regenerating a random app_serial, which the inverter interpreted as a
  session conflict (retcode 0x0001) against the still-cached previous
  session. Daemon now generates ONE app_serial per inverter at startup
  (salted by slot name for multi-inverter uniqueness) and passes it into
  `Session::new_with_app_serial()` on every reconnect.

## 0.1.24 — 2026-04-18

### Attempted
- **Optimistic 0x0001 logon** — experiment to proceed on 0x0001 reply hoping
  queries would succeed. Did not work (queries returned 0xFFFF "LRI not
  available"). Reverted in 0.1.25 in favour of the stable-app_serial fix.

## 0.1.22, 0.1.23 — 2026-04-18

### Fixed
- **Target-BT init filter** — `recv_init_from_target()` loops incoming L2
  replies and only accepts the one whose L1 source BT matches our RFCOMM
  peer. In a MIS network, relay devices answer the broadcast init with
  their own SUSyID; without this filter we'd end up addressing queries
  to a relay that returns 0xFFFF for everything.
- **recv_until_l1_ctrl skips malformed frames** — Frame::parse errors
  inside the recv loop used `?` which tore the session. Now logs hex +
  continues so one bad frame drops one query, not the whole session.

## 0.1.21 — 2026-04-17

### Added
- **Accept first retcode=0 logon reply** — in a MIS multi-inverter network,
  the logon broadcast reaches every device; each replies. Mirror SBFspot's
  `validPcktID` loop: accept the first retcode=0, log the last rejection
  on timeout. Hard-fail on 0x0100 (invalid password).

## 0.1.20 — 2026-04-17

### Added
- **MIS-style `ver\r\n` discovery** (SBFspot.cpp:432) sent before recv-hello
  to warm up the network.
- **5 s reconnect delay** on handshake failure (inverter needs time to
  tear down server side).

## 0.1.19 — 2026-04-17

### Added
- **300 ms gap between logoff and logon** — too-fast back-to-back produced
  retcode 0x0001 on reconnects.
- **Rejected-logon hex dump** for diagnostics when retcode != 0.

## 0.1.18 — 2026-04-17

### Fixed
- **Skip malformed L2 frames** in `recv_l2_with_pkt_id` — one bad frame
  no longer tears the session, now logged + skipped like pkt_id mismatches.

## 0.1.17 — 2026-04-17

### Added
- **Query reply hex dump** so we could reverse-engineer what the inverter
  sends when retcode != 0.
- **L2 replies on any L1 ctrl** initial support.

## 0.1.16 — 2026-04-17

### Fixed
- **Retcode reads from L2 header ErrorCode** (position 22..24 of the L2
  body, aka first "reserved short"). Was reading cmd body [0..2] which
  is zeros → bogus retcode extraction.

## 0.1.14, 0.1.15 — 2026-04-17

### Changed
- **Values parser rewrite** — correct offsets (value at [16..20] for
  28-byte records, u64 at [8..16] for 16-byte counter records), correct
  LRI mask `& 0x00FF_FF00`, NaN sentinels (`0x80000000` i32, `0xFFFFFFFF`
  u32, `0x8000000000000000` u64) return None.
- **LRI constant swap** — `MeteringTotWhOut = 0x00260100` (lifetime, not
  daily), `MeteringDyWhOut = 0x00262200` (daily, not lifetime). Confirmed
  against SBFspot `Types.h`.

## 0.1.12, 0.1.13 — 2026-04-17

### Added
- **pkt_id-matched L2 reply dispatch** — `recv_l2_with_pkt_id` loops up
  to 16 frames discarding replies with wrong pkt_id. Fixes the "stale
  reply from earlier query" bug.

## 0.1.9, 0.1.10, 0.1.11 — 2026-04-17

### Added
- **Skip non-matching L1 frames** for L2 replies (init/logon/query all
  route through `recv_until_l1_ctrl`).
- **Full SBFspot handshake sequence** — hello → echo → topology → init →
  logoff → logon, mirroring `initialiseSMAConnection()`.

## 0.1.7, 0.1.8 — 2026-04-17

### Fixed
- **L2 parse at offset 18 OR 19** — inverter replies place the L2 sig
  directly after the L1 header (offset 18), while our FrameBuilder's
  outbound shape inserts the 0x7E separator first (L2 sig at offset 19).
  Parser now accepts both shapes.

## 0.1.5, 0.1.6 — 2026-04-17

### Fixed
- **L2 wire shape has 0x7E separator** between L1 header and L2 body on
  send. `FrameBuilder::build_l2_wrapped` emits the separator to match
  SBFspot's byte-exact send shape.

## 0.1.4 — 2026-04-17

### Fixed
- **FrameReader length-driven** (not delimiter-scan). L1-only frames have
  no trailing 0x7E; the old scanner over-ran into the next frame's leading
  delimiter and produced length mismatches.

## 0.1.2, 0.1.3 — 2026-04-17

### Changed
- Musl rust builder (rust:1-alpine3.21) so the produced binary runs on
  HA's Alpine base images. glibc builds were silently incompatible.

## 0.1.1 — 2026-04-17

### Added
- Initial clean-room Rust rewrite of SMA BT protocol:
  - `sma-bt-protocol` crate — FCS-16, byte stuffing, L1 + L2 frame encode/decode
  - `bluez-transport` crate — AF_BLUETOOTH RFCOMM client, MockTransport
  - `inverter-client` crate — Session FSM
  - `mqtt-discovery` crate — 7 initial HA sensors
  - `telemetry` crate — Prometheus metrics + tracing
  - `storage` crate — TimescaleDB schema (wiring pending)
  - `daemon` crate — binary entrypoint

0.1.0 internal scaffold only.
