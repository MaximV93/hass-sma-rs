# Changelog

All notable changes to hass-sma-rs are tracked here. This project follows
semantic-ish versioning; pre-1.0 is rapid iteration.

## 0.1.50 — 2026-04-19 (4 new LRIs — CosPhi, nameplate, feed-in limit, active limit)

### Added
- **SpotCosPhi** (LRI range `0x00474800..0x004748FF`) — power factor
  per phase. Live measurement, read every tick. Published as
  `sensor.sbfspot_<slot>_cos_phi` (float 0.000–1.000). Useful for
  grid-quality analysis, reactive-power tracking, DSO dispute
  evidence.
- **NominalAcPower** (LRI `0x00411E00..0x00411E19`) — inverter's
  nameplate AC ceiling. Set at commissioning; static. One-shot query
  at logon. Published as `sensor.sbfspot_<slot>_nominal_ac_power_w`.
- **MaxFeedInPower** (LRI `0x00411F00..0x00411F19`) — configured
  feed-in cap. Shows if the inverter is software-curtailed below its
  nameplate (common in Belgium: 70% of kWp per Fluvius rule, or 50%
  in certain grid-support tariffs). One-shot query at logon.
  Published as `sensor.sbfspot_<slot>_max_feedin_w`.
- **ActivePowerLimit** (LRI `0x00416500..0x00416519`) — live active-
  power limit (where derating is clipping right now). Equals nominal
  when healthy, lower under temperature / grid-voltage derating.
  One-shot query at logon. Published as
  `sensor.sbfspot_<slot>_active_power_limit_w`.

All four are best-effort — a query that fails or returns no parseable
record is silently skipped (debug log only). LRI codes are cited from
SBFspot's `Types.h` but not yet live-validated against the real
SB 3000HF-30 / 2000HF-30 — values may be absent, partial, or need
range adjustment once empirical evidence lands.

### New parsers + 3 regression tests
- `parse_cosphi(body)` — scales i32 value by 1/1000, returns f32
  absolute magnitude.
- `parse_single_watts_record(body, lri_first, lri_last)` — generic
  single-record filter used by the three nameplate/limit queries.
- 84 workspace tests (+3: cosphi scale, cosphi leading-pf absolute,
  lri-range filter). Clippy -D warnings clean.

### 4 new MQTT sensors per device (33 → 37 per inverter)
- `cos_phi` (power_factor class, 3 decimals)
- `max_feedin_w` (power class, W)
- `nominal_ac_power_w` (power class, W)
- `active_power_limit_w` (power class, W)

## 0.1.49 — 2026-04-19 (self-audit follow-up — 6 phases)

Everything from this morning's audit [telegram post] addressed.

### Fixed
- **MIS offline-availability bug** (daemon) — sleep detection now
  flips availability on EVERY announced device, not just the parent
  repeater slot. In MIS setups with 2+ inverters, HA would keep
  showing `available` after the inverter went to sleep because
  `publish_offline` was going to the parent slot that had no
  subscribers.
- **post_yield_deadline staleness** — Option now clears when the
  grace window expires. Behavior unchanged; code smell gone.
- **DeviceCfg.password silent ignore** — now logs WARN at startup
  if someone set it, with an explanation of why broadcast logon
  makes per-device passwords a no-op.

### Added (features)
- **Event-log integration into the daemon** (opt-in via
  `inverters[].event_log_enabled: true`). On enable, queries the
  last 24 h of events once per session, publishes `last_event`,
  `last_event_at`, `last_event_count_24h` MQTT sensors per device.
  Wire behavior still not real-hardware-validated; flag default off.
- **PVOutput HTTP uploader** (opt-in via `--features pvoutput`
  cargo feature — off in default builds to keep binary small).
  Reads from the shared Prometheus registry every
  `upload_interval` (default 5 min) and POSTs to
  `pvoutput.org/service/r2/addstatus.jsp`. Config block
  `pvoutput: { api_key, systems: [...], upload_interval }` on
  DaemonConfig.

### Added (tests)
- MIS multi-device routing integration test (mock transport, 2 target
  serials, verifies both dst_serials reach the outbound L2 header).
- Event-log session roundtrip test (mock transport with fragment +
  sentinel aggregation).
- PVOutput config YAML roundtrip + defaults + empty-systems tests.
- 77 → 81 workspace tests.

### Added (docs)
- **SECURITY.md** — disclosure policy, scope, 24h/7d/30d SLAs.
- **CODE_OF_CONDUCT.md** — short technical-respect code.
- **.github/ISSUE_TEMPLATE/** (bug + feature) + **PR template**.
- **deploy/addon/README.md** — explains the two-repo build flow.
- **ADR 0001 refresh** — added bug #14 (yield LOGOFF + post-yield
  grace) and a topology addendum pointing to ADR 0005.
- **Grafana alerts** — updated to use `{{ $labels.device }}` for
  per-device metrics (imbalance, temperature, grid frequency); added
  label-conventions header comment.
- CHANGELOG entries for every sub-phase + test count fix (76 → 77).

### Internal
- 81 workspace tests, clippy -D warnings clean on BOTH default AND
  `--features pvoutput` builds, rustfmt clean.

## 0.1.48 — 2026-04-19 (event log + PVOutput scaffolding + docs)

### Added
- **Event log query (ADR 0004) — wire layer shipped.** Not yet wired
  into the daemon poll loop (waiting on first live validation against
  a real SB inverter), but everything ELSE is in place:
  - `L2Header::event_log_query` (ctrl=0xE0, ctrl2=0x0100)
  - `build_event_log_body(pkt_id, app_serial, susy, serial, start, end)`
  - `Session::query_event_log_for_device` with multi-packet reply
    aggregation (capped at 32 fragments)
  - `EventRecord` + `parse_event_log_records(body)` — typed 24-byte
    record parser
  - `event_tag_text(tag)` — hand-rolled starter TagList (operation
    states, grid faults, system events, user actions)
  - 4 unit tests covering body shape, record parsing, tag lookup,
    empty-body resilience
- **PVOutput.org scaffolding.** Config types, URL-encoded body
  builder, 5 unit tests. HTTP uploader deliberately deferred to a
  future feature-flagged module so the default binary stays small.

### Docs
- **GETTING_STARTED.md** — 3-scenario walkthrough (single-BT, MIS,
  standalone Linux)
- **ARCHITECTURE.md** — system diagram, crate layout, request
  lifecycle, testing philosophy, extension points
- **CONTRIBUTING.md** — dev setup, style guide,
  reverse-engineering workflow
- **README polish** — new "Getting started" section, refreshed
  badges (76 tests, 0.1.47), updated comparison table and status
  checklist
- 77 workspace tests (includes mis_labels_are_per_device, event-log
  trio, pvoutput quintet), clippy -D warnings clean, rustfmt clean.

## 0.1.47 — 2026-04-19 (probe subcommand + per-device Prometheus)

### Added
- **`hass-sma-daemon probe` subcommand** — one-shot piconet enumeration.
  Connects to a BT MAC, runs the full SMA handshake, prints the
  topology log + every logon reply, then exits with a ready-to-paste
  `inverters: devices:` YAML snippet. No polling, no MQTT, no /data
  writes. Solves the painful "which serials are behind this repeater?"
  discovery problem without the restart-churn that stranded sessions
  on 2026-04-19.

  Usage:
  ```
  hass-sma-daemon probe --mac 00:80:25:AA:BB:CC --password 0000
  ```

- **Per-device Prometheus labels** — MIS multi-device metrics now carry
  both `slot` (RFCOMM session slot) AND `device` (the actual
  inverter). Session-level counters (bt_reconnects_total,
  handshake_errors_total, etc.) keep single-label `slot`. Legacy
  single-device installs unchanged: `slot == device` in that mode so
  old Grafana queries still work.

### Documented
- `DeviceCfg.password` field is explicitly documented as UNUSED —
  SMA's broadcast logon uses one password for all devices on the
  piconet, so per-device password only matters if we adopt a
  per-device re-logon model (not planned). Field kept for schema
  stability + future optionality.

## 0.1.46 — 2026-04-19 (MIS multi-inverter polling)

### Added
- **Multi-device polling behind one BT repeater.** Optional
  `devices:` sub-list on each `inverters:` config entry enumerates
  the inverters reachable via a shared BT repeater — SBFspot's MIS
  model. Each entry produces its own 28-sensor HA device card and
  gets polled round-robin every tick through the same RFCOMM session.
  Empty `devices:[]` stays backwards-compatible (legacy single-device
  path). See [ADR 0005](docs/adr/0005-mis-multi-inverter.md).
- `Session::query_for_device(susy, serial, kind)` — addressable
  query primitive. `Session::query(kind)` now delegates to it using
  the init-derived identity.
- `InverterCfg.devices: Vec<DeviceCfg>` + YAML schema + 2 unit tests.
- Per-device `announce` + `publish_online` + firmware/model queries
  in the daemon's post-logon setup.
- Per-device `poll_status` / `last_poll` / `session_uptime` on every
  tick, so HA alerts work per device.

### Example MIS config (Maxim's installation)
```yaml
inverters:
  - slot: repeater
    bt_address: "00:80:25:21:32:35"
    password: "<shared>"
    devices:
      - slot: zolder
        app_serial: 2120121246
        model: "SB 3000HF-30"
      - slot: garage
        app_serial: 2120121383
        model: "SB 2000HF-30"
```

### Internal
- 66 workspace tests green, clippy -D warnings clean.
- Addon schema extended with `devices:` optional list.

## 0.1.45 — 2026-04-19 (persistent app_serial across restarts)

### Fixed
- **app_serial now persists across addon restarts.** Previously it was
  regenerated from `SystemTime::now() + slot_hash` at each startup.
  Every addon restart → new app_serial → inverter sees an unfamiliar
  client while still holding the PREVIOUS session (app_serial tracked
  for ~15 min) → logon rejected with retcode 0x0001
  ("session already active"). Two-inverter piconets are particularly
  affected because BOTH inverters hold the stale session state.
- Now stored at `/data/app_serial_<slot>` (HA-persistent, survives
  `ha addons update` and OS-level reboots). Fallback to the old
  time+hash minting on first run / if the file is missing or
  corrupted. Write failures log a warning but don't abort.

Caught the hard way while probing MIS peer MACs — restarting the
addon a few times in a row left zombie sessions on both zolder and
garage inverters, locking everyone out for 10+ minutes until the
inverter-side cache expired.

## 0.1.44 — 2026-04-19 (MIS discovery groundwork)

### Added
- **INFO log on every logon reply** — on an MIS network the logon
  broadcast hits multiple devices and each replies. The accepted one
  was the only one previously surfaced; rejections went to DEBUG. Now
  every reply logs at INFO with `susy`, `serial`, `code`, `accepted`
  — useful as a de-facto piconet enumeration per session start, and
  as groundwork for the planned multi-inverter support
  (see [ADR 0005](docs/adr/0005-mis-multi-inverter.md)).

No behavior change — still breaks on first retcode=0 logon reply,
still rejects on 0x0100.

## 0.1.43 — 2026-04-19 (post-yield grace window)

### Fixed
- **EHOSTDOWN after yield no longer escalates to 10-min sleep backoff.**
  0.1.42 added a clean LOGOFF before the yield-window drop but that
  alone did not fix the reconnect failure — observed live at 07:54
  local: LOGOFF was sent, yield elapsed, 3× `Host is down` errors, and
  the adaptive-backoff correctly-but-unhelpfully flagged the inverter
  as "asleep" → 10 min of sensors `unavailable`. Root cause is BT
  link-layer: the SB 3000HF-30's BT radio takes up to a minute to
  re-advertise after a clean disconnect, even in bright daylight.
- New `post_yield_deadline` state: after every intentional yield the
  reconnect loop enters a 180 s grace window during which EHOSTDOWN
  triggers a short `POST_YIELD_RETRY` (15 s) instead of incrementing
  the sleep-streak. Cleared as soon as any connect succeeds. Grace
  window also preserved until connect success, so if post-yield
  EHOSTDOWN persists past 180 s the code falls back to the normal
  sleep-detection ladder (genuine protocol breakage, not a yield
  artifact).

## 0.1.42 — 2026-04-19 (sunrise bug hunt)

### Fixed
- **Yield-window teardown sent no LOGOFF** — the parallel-run feature
  dropped the RFCOMM socket abruptly, leaving the inverter holding
  zombie session state for ~15 min. Every post-yield reconnect inside
  that window failed with `EHOSTDOWN (os error 112)`, which the
  adaptive-backoff mistook for "inverter asleep" → 10-minute reconnect
  delays → sensors stuck `unavailable` for 15+ min after every yield.
  Caught at dawn today when the first yield-after-night-idle triggered
  it live. New `Session::graceful_close()` sends a proper LOGOFF
  (broadcast dst, L1 ctrl=0x0001, same shape SBFspot uses in
  `logoffSMAInverter()`) with a 150ms grace period before closing.
  Used on both the yield path and the normal end-of-outer-loop close.
- **MockTransport is now `Clone`** — backed by `Arc<Mutex>` so tests
  can keep a handle after moving one into `Session::new`. Needed for
  the two new regression tests.

### Added
- **Regression tests**:
  - `graceful_close_emits_logoff` — after handshake, exactly one
    extra frame is sent on close, with the L2 signature present
    and broadcast dst.
  - `graceful_close_safe_when_not_logged_in` — no send is attempted
    if the session never reached `LoggedIn`. Safe on error paths.
  64 workspace tests total (up from 62).

## 0.1.40 — 2026-04-19 (overnight work)

### Added
- **Session Uptime** diagnostic sensor (28 total) — seconds since last
  successful logon. Useful for "detect session flapping" alerts.
- **Archive sink** (opt-in via `archive:` config) — TimescaleDB OR CSV.
  CSV sink is zero-config, writes one file per (slot, YYYY-MM-DD).
  TimescaleDB reuses the existing hypertable + retention + continuous
  aggregate schema.
- **Grafana alerts** — 7 unified-alerting rules (`docs/grafana-alerts.yaml`)
  covering daylight offline, stale poll, handshake rate, string
  imbalance, overheating, grid frequency, reconnect storm.
- **Hardened systemd unit** — full `systemd.exec(5)` sandboxing with
  PrivateUsers, SystemCallFilter, DeviceAllow-list, MemoryMax=128M.
  Install docs at `deploy/systemd/README.md`.
- **ADR docs** — 4 architecture decision records (protocol narrative,
  persistent session, Rust choice, event log deferral) with index
  at `docs/adr/README.md`.
- **Frame parser fuzzing** — 6 proptest properties with 1536 random
  iterations per run. `parse_never_panics_on_random_bytes`, length
  mutation safety, truncation safety, header checksum robustness.
  62 workspace tests (up from 54).

### Fixed
- Clippy `-D warnings` clean under Rust 1.95 (repeat_n,
  is_multiple_of, RangeInclusive::contains suggestions applied).

## 0.1.39 — 2026-04-19
Same content as 0.1.40 — bump triggered rebuild after storage crate
was wired into the daemon's main.rs.

## 0.1.38 — 2026-04-18

### Added
- **BT piconet topology log** — on every handshake the daemon logs every
  BT device seen in the inverter's piconet with its MAC address and role
  flag. Drastically simplifies finding additional inverter MACs (no more
  inverter-LCD menu spelunking). Verified live: zolder piconet reports
  3 inverter devices + our local BT host.

## 0.1.37 — 2026-04-18

### Fixed
- **TypeLabel tag map corrected** — SBFspot's TagListEN-US.txt confirms
  9073 = SB 3000HF-30 (my initial guess 9072 was wrong; 9072 is the SB
  2500HF-30). Expanded lookup to cover 9070-9086 SB HF/TL family.
- **Config-model fallback** — daemon now publishes identity.model from
  config BEFORE the TypeLabel query runs, so HA immediately shows the
  correct model string. TypeLabel only overrides if the reply has a
  known tag; unknown tags no longer replace the good value with a
  "TagID <n>" placeholder.

## 0.1.36 — 2026-04-18

### Internal
- Force rebuild to ensure the 40-byte TypeLabel parser is in the image.

## 0.1.35 — 2026-04-18

### Fixed
- **TypeLabel records are 40 bytes** (not the standard 28), with 8
  attribute slots at offsets 8..40. Tag id is in low 24 bits, high byte
  0x01 marks selected, 0xFE is end-of-list. Was previously reading
  `u32_value_28(offset 16)` which produced garbage like "TagID 842084913".
- **status_text table expanded to 36 entries** sourced from SBFspot
  TagListEN-US.txt (operation health + grid relay + common faults).

## 0.1.34 — 2026-04-18

### Added
- **Inverter Model sensor** — publishes `type_label_text(tag)` lookup
  result from the TypeLabel query. New diagnostic entity in HA.
  Falls through to `TagID <n>` for unknown tag ids.

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
