# Architecture

How hass-sma-rs is structured, why it's structured that way, and how a
request flows end-to-end.

## System view

```
                      ┌────────────────────────────────────────┐
                      │            Home Assistant             │
                      │  (MQTT broker + UI + Recorder + LTS)   │
                      └────────────┬───────────────────────────┘
                                   ▲  MQTT publish (discovery + state)
                                   │  port 1883
                                   │
      Prometheus ◀── HTTP GET ── ┌─┴─────────────────────────────┐
     /metrics :9090              │    hass-sma-daemon (Rust)     │
                                 │                               │
                                 │  [tokio task per inverters:   │
                                 │   entry] — one RFCOMM socket  │
                                 │   per entry, round-robin over │
                                 │   configured devices.         │
                                 │                               │
                                 │  tracing JSON logs → stdout   │
                                 │  /data/app_serial_<slot>.bin  │
                                 │  /share/hass-sma-rs/*.csv (opt)│
                                 └───────────────┬───────────────┘
                                                 │  AF_BLUETOOTH
                                                 │  RFCOMM (port auto)
                                                 ▼
                              ┌───────────────────────────────────┐
                              │       SMA BT endpoint             │
                              │  (single inverter OR repeater +   │
                              │   N inverters on internal bus)    │
                              └───────────────────────────────────┘
```

## Crate layout

```
crates/
├── sma-bt-protocol     # Pure-logic: wire format, frames, FCS-16,
│                       # byte stuffing, L2 headers, command opcodes.
│                       # No I/O. No dependencies outside byteorder.
│                       # 21 unit tests + 3 captured-frame tests +
│                       # 6 proptest fuzz properties.
├── bluez-transport     # AF_BLUETOOTH RFCOMM client + MockTransport +
│                       # FrameReader. The transport trait Session
│                       # consumes. Mock tests don't need real BT.
├── inverter-client     # Session FSM. Handshake → logon → query loop.
│                       # Multi-packet reply collection (event log).
│                       # Ignores the difference between direct-BT
│                       # and MIS — both look the same at this layer.
├── mqtt-discovery      # MQTT client + HA discovery message builder.
│                       # One `Publisher` per InverterIdentity.
│                       # 28-sensor catalog.
├── telemetry           # Prometheus registry + /metrics HTTP server
│                       # (hyper) + tracing_subscriber init helpers.
├── storage             # Optional archive sinks. Two backends:
│                       # TimescaleDB (sqlx + hypertable + continuous
│                       # aggregates) and a zero-config CSV-per-day
│                       # fallback.
└── daemon              # Binary. Parses config, spawns per-inverter
                        # tasks, wires Sessions into Publishers +
                        # metrics + archiver, handles signals.
```

## Request lifecycle (one poll tick)

```
1.  Ticker fires in run_inverter              [daemon]
2.    For each target in targets:              [daemon]
3.      For each QueryKind:                    [daemon]
4.        Session::query_for_device(susy,
            serial, kind)                       [inverter-client]
5.          build_query_body                    [sma-bt-protocol]
6.          FrameBuilder::build (L2-wrapped,
              FCS-16, byte-stuffed)              [sma-bt-protocol]
7.          Transport::send_frame                [bluez-transport]
            → kernel RFCOMM → BT air → inverter
            ← inverter reply ← BT air ← kernel
8.          Transport::recv_frame                [bluez-transport]
9.          Frame::parse + decode_l2             [sma-bt-protocol]
10.         recv_l2_with_pkt_id filters on
              pkt_id match                        [inverter-client]
11.       publish_query_result parses typed      [daemon]
            values + publishes to MQTT + updates
            Prometheus + archive
12.   Update poll_status=ok, last_poll,
        session_uptime                           [daemon]
13. Next tick.
```

Steps 1-13 for the 11-query per-tick batch complete in ~3-8 seconds,
well inside the default 60-second poll interval.

## Why persistent session?

Stock SBFspot reconnects per poll: for every 60-second tick it
hello → echo → topology → init → logoff → logon → queries.
Handshake cost ≈ 5 s every tick.

hass-sma-rs does the handshake ONCE per daemon startup (or after a
real error), then reuses the RFCOMM socket + session state across
thousands of ticks. Observed live on 2026-04-19: **4 hours of
continuous session, 240+ poll cycles, 0 reconnects.**

Tradeoffs documented in [ADR 0002](adr/0002-persistent-bluetooth-session.md).

## Why Rust?

Stock SBFspot is ~12.5 kLOC of C++ that historically had buffer
overruns on pcktBuf + integer narrowing + unchecked pointer math in
`get_long(pcktBuf + N)`. Rust's type system rules out those by
construction. Full rationale in [ADR 0003](adr/0003-rust-over-cpp.md).

## Why MIS via one session?

SMA's BT repeater fronts multiple inverters over an internal bus. At
the protocol level, every L2 request addresses a specific
`(susy_id, serial)` — the repeater routes. Opening multiple RFCOMM
sockets to the repeater MAC fails with EBUSY (only one master socket
per BT endpoint). So multi-device MIS means: one RFCOMM, N logical
targets iterated per tick.

Design + evidence in [ADR 0005](adr/0005-mis-multi-inverter.md).

## Testing philosophy

1. **Protocol is fully unit-testable** — no I/O dependencies in
   sma-bt-protocol. Every bug found in reverse-engineering has a
   regression test using the exact captured bytes that triggered it.
2. **Session uses MockTransport** — happy-path handshakes, logon
   rejection, graceful close are verified without real BT.
3. **Fuzz the parser** — 6 proptest properties with 1536 random
   iterations per property. Guarantees Frame::parse never panics on
   adversarial input, roundtrips are lossless, truncation is safe,
   header checksum mutations are caught.
4. **Captured-frame fixture** — 290 real frames from live captures
   committed under `tests/fixtures/captured/`. Every decoder change
   replays these.

Run the full suite: `cargo test --workspace`. Today: 76 tests + proptest,
all green on Rust stable 1.95.

## Extension points

Where contributors typically need to change things:

| Goal | File(s) |
|---|---|
| Add a new spot-query kind | `crates/sma-bt-protocol/src/commands.rs` (opcode), `crates/inverter-client/src/values.rs` (parser), `crates/mqtt-discovery/src/catalog.rs` (HA sensor), `crates/daemon/src/main.rs` (publish) |
| New alerting rule | `docs/grafana-alerts.yaml` (Prometheus-side) + `crates/telemetry/src/metrics.rs` (if a new metric is needed) |
| Additional protocol variant (e.g. Ethernet-mode SMA) | `crates/bluez-transport/` (new Transport impl) + possibly `crates/sma-bt-protocol/src/constants.rs` (ETH_L2_SIGNATURE) |
| Per-device observability split | See `DeviceLabels` in `crates/telemetry/src/metrics.rs` |

## Non-goals

- **Not a general MQTT gateway.** We only publish SMA readings to HA;
  we don't subscribe to anything.
- **Not a PV-grid writer.** Inverters can be configured remotely via
  SMA's protocol (modbus writes to VRT-equivalent LRIs) but that's
  out of scope — risk/reward is bad for a home install.
- **Not cross-vendor.** Fronius, SolarEdge, Enphase all speak
  different protocols. hass-sma-rs is SMA-only.
