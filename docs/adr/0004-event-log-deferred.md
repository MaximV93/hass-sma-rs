# ADR 0004 — Event log query deferred to a later phase

Date: 2026-04-19
Status: deferred

## Context

SBFspot's `ArchData.cpp:GetEventData()` implements a time-range query that
returns the inverter's event log (every fault, warning, info event with
its tag id + timestamp + device context). haos-sbfspot exposes this via
the event_data CSV export. This is the ONE stock-SBFspot feature I
explicitly skipped today.

## Why it's harder than a spot query

Every spot query (`QueryKind::SpotAcTotalPower` etc) sends a single L2
body of `opcode(4) | lri_start(4) | lri_end(4) = 12 bytes`, gets a
single reply with N records. Event-data queries:

1. Use command `0x7010_0200` (user) or `0x7012_0200` (installer) — the
   0x70xx range is ArchData, distinct from the 0x51xx..0x58xx spot
   queries.
2. Use `ctrl=0xE0` in the L2 header (not 0xA0 like spot queries).
3. Body is `opcode(4) | start_time_u32(4) | end_time_u32(4)` — time
   range instead of LRI range.
4. Reply is **multi-packet** — the inverter fragments the event list
   across several frames. You know you're done when a frame comes back
   with `fragment_id = 0`. Each frame's L2 header has a non-zero
   fragment_id until the last one.
5. Record size is 24 bytes (not 28), with a different layout:
   ```
   [0..4]   entry_id (u32 seq)
   [4..8]   timestamp (u32 unix)
   [8..12]  event_code (u32, with tag in low 16)
   [12..16] SUSyID + serial of the reporting device
   [16..20] optional param 1
   [20..24] optional param 2
   ```
6. Event tag → text lookup via SBFspot's TagList is necessary to get
   human-readable messages. Our existing `status_text()` subset covers
   ~36 of the ~1500 tags — we'd want a broader lookup loaded from the
   TagListEN-US.txt at runtime.

## Decision

Defer to a later session. Rationale:

- Zero impact on live solar monitoring (event log is historical
  diagnostics, not real-time data).
- Multi-packet reply path needs to be added to the session layer —
  materially different from the single-reply path we have. Would need
  ~500 new LOC in `inverter-client::session::query_arch_data`.
- The entire TagList lookup table (~1500 entries) would need to ship
  with the crate, adding non-trivial binary size.
- Can't validate tonight without daylight BT.

## What's needed to resurrect

1. Add `L2Header::arch_data(pkt_id, start, end)` with `ctrl=0xE0`,
   `longwords=0x0F`.
2. Add `build_event_log_body(pkt_id, app_serial, start, end)` in
   auth.rs (parallel to build_query_body).
3. Add `Session::query_multipacket(body) -> Vec<Reply>` that loops
   recv until fragment_id=0.
4. Add `values::parse_event_entries(&[u8]) -> Vec<EventEntry>`.
5. Ship a TagList as a `const` or `LazyLock<HashMap<u32, &str>>` (large
   — consider gating behind a `events` cargo feature).
6. Add a CSV export path in the archive sink (events are fundamentally
   different shape — timestamped discrete entries, not periodic
   metrics).
7. Optional: publish to MQTT as a "last event" sensor + attributes
   dict for recent history.

## Consequences

- Users who need full inverter event log continue to use haos-sbfspot
  (MariaDB archive mode). Documented in COMPARISON.md.
- Alerting on recent errors is still possible via `inverter_state`
  sensor (reports current health tag from `DeviceStatus` query, which
  we already support).
- When we resurrect this, ADR 0004 becomes "superseded by 0005 — event
  log implemented" and links to the implementation PR.
