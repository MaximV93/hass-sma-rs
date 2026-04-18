# ADR 0002 — Persistent BT session vs reconnect-per-poll

Date: 2026-04-18
Status: accepted

## Context

Stock SBFspot and haos-sbfspot open a fresh RFCOMM connection, do the
full handshake, query, then disconnect on every poll. At a 60-second
poll interval, each cycle spends ~5 seconds in handshake + logon (the
inverter takes 2-3s just to respond to the initial "ver" discovery)
plus ~3s of query time. That's ~8s of BT airtime per minute, 13% of
realtime used on a single inverter, more if polling faster or if you
have multiple inverters.

For a multi-inverter setup (user has 2 inverters), this scales linearly:
two back-to-back handshakes per poll cycle.

## Decision

hass-sma-rs keeps the RFCOMM session open between polls. One handshake +
logon at startup, then N query cycles, and only teardown+reconnect on
actual errors.

The `Session` type in `inverter-client` owns the handshake state and
provides a `query()` method callers can hit repeatedly. The daemon's
per-inverter tokio task runs a simple `loop { ticker.tick(); session.query_sweep() }`.

## Consequences

### Good

- **~5s saved per poll** on amortized handshake cost.
- **Faster poll intervals possible** — 5s poll is trivially supported
  (2s sweep + 3s idle), whereas stock SBFspot at 5s would be 100% BT.
- **More stable session state** — the inverter's session-conflict
  detection (retcode=0x0001) is avoided entirely because we don't
  re-logon.
- **Lower RF power draw** — continuous RFCOMM is a low-duty-cycle
  stream; rebuilding the connection is power-hungry (page scan, SDP,
  authentication).

### Bad

- **BT exclusivity** — we hold hci0 continuously, blocking any other
  SMA integration (haos-sbfspot) from using it. Addressed by
  `yield_every` + `yield_duration` config so users doing parallel-run
  migration can share.
- **Longer downtime on a crash** — if the daemon dies mid-session,
  the inverter may take up to 900s (our logon timeout) to expire the
  stale session. Mitigated by:
  - `app_serial` persisted across restarts → next logon is accepted as
    a continuation, not a conflict
  - graceful `logoffSMAInverter` sent on clean shutdown
- **Complex state tracking** — `Session` needs to track its own FSM
  (Disconnected / Handshaking / LoggedIn) and recover from transient
  frame errors without tearing down.

### Neutral

- Session state `inverter_susy_id` + `inverter_serial` only populated
  AFTER init reply. Queries before logon would use broadcast dst,
  which is fine for most commands but wastes the BT airtime on
  unnecessary broadcasts.

## Alternatives considered

**Reconnect-per-poll (SBFspot-style)**. Rejected because of the 5s/poll
tax and the inability to poll faster than the handshake duration. Also
makes multi-inverter much harder because session state is never cached.

**Connection pool**. Overkill for a device that supports exactly one
RFCOMM channel per inverter.

**Event-driven (push-only, no polling)**. The inverter DOES emit
unsolicited `ctrl=0x0008` push frames with current spot data, but
unreliably and only for some LRIs. Can't be the only data source.
Could be a future optimization (parse pushes + fall back to polling).

## Validation

Live-observed at 2026-04-18 19:42–22:21 local:
- 100+ successful polls in a single session
- 0 session teardowns after the stable-app_serial fix (ADR 0001 §6)
- Intermittent inverter sleep handled by adaptive backoff without
  losing the session state

## References

- `crates/inverter-client/src/session.rs` — Session FSM
- `crates/daemon/src/main.rs:run_inverter` — poll loop
- ADR 0001 §6 — why stable app_serial is required for this decision
  to work
