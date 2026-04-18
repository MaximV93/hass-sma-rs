# hass-sma-rs

[![CI](https://github.com/MaximV93/hass-sma-rs/actions/workflows/ci.yaml/badge.svg)](https://github.com/MaximV93/hass-sma-rs/actions/workflows/ci.yaml)
![tests](https://img.shields.io/badge/tests-62%20green-brightgreen)
![license](https://img.shields.io/badge/license-MIT%20%7C%20Apache--2.0-blue)
![rust](https://img.shields.io/badge/rust-stable-orange)

Clean-room Rust implementation of the SMA Sunny Boy Bluetooth
integration for Home Assistant. Replaces the bash + SBFspot C++ stack
with a single type-safe daemon.

**Live since 2026-04-18.** 38+ addon versions iterated against a real
SB 3000HF-30. 12 protocol bugs reverse-engineered and fixed with
regression tests. 27 MQTT sensors + 15 Prometheus metrics per inverter.

## Why

| | Stock SBFspot (C++) | haos-sbfspot (fork) | hass-sma-rs |
|---|---|---|---|
| BT session | reconnect per poll | reconnect per poll | **persistent** |
| Handshake tax | ~5 s / poll | ~5 s / poll | 1 s **once** |
| MQTT LWT | ❌ | ❌ | ✅ sensors flip `unavailable` on crash |
| Sleep detection | ❌ | ❌ | ✅ adaptive backoff after sunset |
| Metrics | ❌ | MQTT heartbeat | ✅ **15 Prometheus families** |
| Parallel-run | ❌ | ❌ | ✅ `yield_every` window |
| Tests | ❌ | bash smoke only | ✅ **62** unit + integration + fuzz |
| Event log | ✅ | ✅ via MariaDB | ⏳ deferred — see [ADR 0004](docs/adr/0004-event-log-deferred.md) |

Full matrix + measurement methodology: [docs/COMPARISON.md](docs/COMPARISON.md).

## Quick start (HA addon)

```
Supervisor → Add-on store → ⋮ Repositories → Add
https://github.com/MaximV93/hassio-addons

Install "hass-sma-rs (Rust rewrite)" → Configure → Start.
```

Config skeleton:

```yaml
inverters:
  - slot: zolder
    bt_address: "00:80:25:AA:BB:CC"
    password: "<inverter user password>"
    poll_interval: 60s
    model: "SB 3000HF-30"
```

## Quick start (standalone Linux)

Raspberry Pi, Proxmox LXC, bare-metal homelab:

```bash
cargo build --release
sudo install -m 755 target/release/hass-sma-daemon /usr/local/bin/
sudo install -m 644 deploy/systemd/hass-sma-rs.service /etc/systemd/system/
# edit /etc/hass-sma-rs/config.yaml
sudo systemctl enable --now hass-sma-rs
```

Details: [deploy/systemd/README.md](deploy/systemd/README.md).

## Architecture

```
crates/
  sma-bt-protocol/     wire protocol: frames, FCS-16, byte stuffing,
                       L2 packet header, command opcodes, authentication
  bluez-transport/     AF_BLUETOOTH RFCOMM client + MockTransport + FrameReader
  inverter-client/     Session FSM (Handshaking → Enumerating → LoggedIn → Polling)
  mqtt-discovery/      HA MQTT discovery publisher (28-sensor catalog, LWT)
  telemetry/           Prometheus /metrics + tracing JSON logs
  storage/             TimescaleDB writer + CSV sink (optional)
  daemon/              tokio-based per-inverter task runner (the binary)

deploy/
  addon/               HA addon wrapper (Dockerfile + config.yaml)
  systemd/             standalone Linux unit file + install README

docs/
  COMPARISON.md        three-way feature matrix
  adr/                 architecture decision records (why persistent session,
                       why Rust, why event log deferred)
  grafana-dashboard.json
  grafana-alerts.yaml
```

## Status

**0.1.40**. Live at a production HA instance since 2026-04-18.

- [x] Core protocol: L1 + L2 parse/build, FCS-16, byte stuffing
- [x] Handshake + logon (full SBFspot MIS-style sequence)
- [x] 11 query kinds with typed parsers, 28 MQTT sensors
- [x] MQTT LWT availability
- [x] Adaptive sleep backoff + EHOSTDOWN detection
- [x] Prometheus `/metrics` on :9090
- [x] Parallel-run yield window for coexistence with other SMA integrations
- [x] Archive sink: TimescaleDB or CSV (opt-in)
- [x] 62 tests (unit + integration + proptest fuzz)
- [x] Hardened systemd unit
- [x] 4 ADRs documenting major decisions
- [ ] Event log query (deferred, [ADR 0004](docs/adr/0004-event-log-deferred.md))
- [ ] PVOutput upload (low priority, easy to add)

## License

SBFspot's CC BY-NC-SA source used as protocol reference only — no code
copied. Captured wire frames (in `tests/fixtures/captured/`) are from my
own inverter and are distributed here under Apache-2.0.

Rust code: MIT OR Apache-2.0 at your option.

## Credits

Protocol reverse-engineering based on observation of the SMA wire format
and the public [SBFspot](https://github.com/SBFspot/SBFspot) reference
implementation. PPP FCS-16 per RFC 1662 §C.2.
