# hass-sma-rs

[![CI](https://github.com/MaximV93/hass-sma-rs/actions/workflows/ci.yaml/badge.svg)](https://github.com/MaximV93/hass-sma-rs/actions/workflows/ci.yaml)
![tests](https://img.shields.io/badge/tests-76%20green-brightgreen)
![license](https://img.shields.io/badge/license-MIT%20%7C%20Apache--2.0-blue)
![rust](https://img.shields.io/badge/rust-1.75%2B-orange)
![version](https://img.shields.io/badge/version-0.1.47-blue)

Clean-room Rust implementation of the SMA Sunny Boy Bluetooth
integration for Home Assistant. Replaces the bash + SBFspot C++ stack
with a single type-safe daemon.

**Live since 2026-04-18.** 47 addon versions iterated against real
SB 3000HF-30 + SB 2000HF-30 inverters behind a BT repeater (MIS mode).
14 protocol bugs reverse-engineered and fixed with regression tests.
29 MQTT sensors per device + 15 Prometheus metric families.

## Getting started

**New user?** → [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md) —
5-minute walkthrough for HA addon, MIS multi-device, or standalone
Linux setups.

**Contributing?** → [CONTRIBUTING.md](CONTRIBUTING.md) — dev setup,
test philosophy, reverse-engineering workflow.

**Architecture deep-dive?** → [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
— crate layout, request lifecycle, design rationale.

## Why

| | Stock SBFspot (C++) | haos-sbfspot (fork) | hass-sma-rs |
|---|---|---|---|
| BT session | reconnect per poll | reconnect per poll | **persistent** |
| Handshake tax | ~5 s / poll | ~5 s / poll | 1 s **once** |
| MQTT LWT | ❌ | ❌ | ✅ sensors flip `unavailable` on crash |
| Sleep detection | ❌ | ❌ | ✅ adaptive backoff after sunset |
| Metrics | ❌ | MQTT heartbeat | ✅ **15 Prometheus families**, per-device labels |
| MIS multi-inverter | ✅ | ✅ | ✅ **`devices:` round-robin in one session** |
| Parallel-run | ❌ | ❌ | ✅ `yield_every` window |
| Restart-safe session identity | ❌ | ❌ | ✅ `/data/app_serial_<slot>` persisted |
| Probe subcommand | ❌ | ❌ | ✅ `hass-sma-daemon probe --mac ...` |
| Tests | ❌ | bash smoke only | ✅ **76** unit + integration + proptest fuzz |
| Event log | ✅ | ✅ via MariaDB | ⏳ wire-layer shipped, daemon integration pending |

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

**0.1.47** live at a production HA instance since 2026-04-18. 2 SB
inverters monitored via MIS (zolder SB 3000HF-30 + garage SB 2000HF-30
behind shared BT repeater). Daily production visibility recovered +
improved.

- [x] Core protocol: L1 + L2 parse/build, FCS-16, byte stuffing
- [x] Handshake + logon (full SBFspot MIS-style sequence, 14 bugs reverse-engineered)
- [x] 11 query kinds with typed parsers, 29 MQTT sensors per device
- [x] MIS multi-inverter support (`devices:` round-robin, ADR 0005)
- [x] Persistent app_serial (`/data/app_serial_<slot>` — survives addon restarts)
- [x] Probe subcommand (`hass-sma-daemon probe --mac ...`) for safe enumeration
- [x] Graceful LOGOFF on yield + post-yield grace window
- [x] MQTT LWT availability per device
- [x] Adaptive sleep backoff + EHOSTDOWN detection
- [x] Prometheus `/metrics` on :9090 with per-device labels
- [x] Archive sink: TimescaleDB or CSV (opt-in)
- [x] 76 tests (unit + integration + proptest fuzz)
- [x] Hardened systemd unit
- [x] 5 ADRs documenting major decisions
- [x] Event log: wire layer, parser, TagList — daemon integration pending live-wire validation
- [x] PVOutput: body builder + config types — HTTP uploader pending

## License

SBFspot's CC BY-NC-SA source used as protocol reference only — no code
copied. Captured wire frames (in `tests/fixtures/captured/`) are from my
own inverter and are distributed here under Apache-2.0.

Rust code: MIT OR Apache-2.0 at your option.

## Credits

Protocol reverse-engineering based on observation of the SMA wire format
and the public [SBFspot](https://github.com/SBFspot/SBFspot) reference
implementation. PPP FCS-16 per RFC 1662 §C.2.
