# hass-sma-rs

Clean-room Rust rewrite of the SMA Sunny Boy BT-only integration for Home
Assistant. Replaces the bash + SBFspot-based fork at
[MaximV93/hassio-addons](https://github.com/MaximV93/hassio-addons/tree/main/haos-sbfspot)
incrementally while keeping the previous fork (`v-lastworking-2026.4.17.18`)
as a parallel-run reference.

## Why

The fork works. It's also 500+ lines of bash wrapping a C++ binary whose BT
session model forces 20-40s handshake per poll, makes sub-minute polling
saturated, and leaves every edge case (hangs, BT stack corruption, retention,
supply chain) as a patch on top.

Rust rewrite goals:

- **Persistent BT sessions** — open once, keep alive, reconnect on failure.
  Removes the handshake bottleneck that makes 5 s polling saturated today.
- **Observable by construction** — `tracing` spans, Prometheus metrics, OTLP
  traces as first-class, not MQTT-proxy-as-metrics.
- **Testable by construction** — protocol in a pure-function crate; transport
  behind a trait mock; integration tests replay captured wire traffic.
- **Typed config + typed protocol** — no stringly-typed `jq` on
  `options.json`, no shell-level string munging on wire frames.
- **Single static binary** — distroless runtime, no Alpine + bashio + s6.

Non-goals: SMA Speedwire / Ethernet / Webconnect. HF-30 hardware is
BT-only — this daemon is explicitly for that class of inverter.

## Layout

```
crates/
  sma-bt-protocol/     # clean-room impl of the SMA BT wire protocol
                       # (frames, FCS-16, byte stuffing, commands)
  bluez-transport/     # RFCOMM socket wrapper + mock for tests
  inverter-client/     # per-inverter state machine + session lifecycle
  telemetry/           # tracing + prometheus + otlp setup
  storage/             # TimescaleDB writer, MariaDB migration tool
  mqtt-discovery/      # HA discovery publisher (owns all sensors)
  daemon/              # main binary
deploy/
  addon/               # HA addon wrapper (Dockerfile + config.yaml)
  systemd/             # standalone-mode unit file
docs/
  adr/                 # architecture decision records
```

## Status

**V5.1 scaffold.**

- [x] Cargo workspace + stub crates
- [x] `sma-bt-protocol`: frame encoder, parser, FCS-16, byte stuffing
      (9/9 unit tests pass, verified against PPP FCS-16 reference vectors)
- [ ] `sma-bt-protocol`: L2 packet body builder + command opcodes
- [ ] `sma-bt-protocol`: authentication handshake
- [ ] `bluez-transport`: RFCOMM socket
- [ ] `bluez-transport`: mock for integration tests
- [ ] `inverter-client`: session FSM
- [ ] `mqtt-discovery`: own all per-inverter sensors
- [ ] `storage`: TimescaleDB schema + writer
- [ ] `daemon`: wire everything up
- [ ] Parallel-run diff vs SBFspot fork
- [ ] Cutover

## License

SMA's proprietary BT protocol is observed at the wire level; the SBFspot
reference implementation (CC BY-NC-SA 3.0) was used as documentation only.
No SBFspot source code is copied into this repository.

Rust code in this repository is licensed MIT OR Apache-2.0 at your option.
