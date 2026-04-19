# ADR 0005 — MIS multi-inverter polling (repeater + N devices)

Date: 2026-04-19
Status: planned

## Context

Maxim's SMA installation is an MIS network, not two independent direct-BT
inverters as originally assumed. Real topology:

```
hci0 (HA) ←BT→ BT Repeater @ 00:80:25:21:32:35
                 ├── SB 3000HF-30 "zolder"  serial 2120121246
                 └── SB 2000HF-30 "garage"  serial 2120121383
```

Evidence: `/share/sbfspot-logs/sbfspot-2026-04-18.log` shows ONE BT
connection target (`Connecting to 00:80:25:21:32:35`) and THREE distinct
device identities on it: the repeater itself (serial 170004597,
class="Communication products", type="Bluetooth Repeater") plus the two
inverters. SBFspot's MIS mode distinguishes them by app_serial in each
request.

hass-sma-rs today is single-device: `Session::query(kind)` uses
`self.inverter_serial` captured from the init reply, and that serial
is fixed for the lifetime of the session. Opening a second
`inverters:` entry with the same `bt_address` would hit EBUSY on the
second RFCOMM connect → we cannot just scale horizontally to reach
garage.

## Decision

Add explicit MIS multi-device support as a session-level capability
(ONE RFCOMM, N logical devices addressed via app_serial in each
request body). Config-driven (user supplies serials) rather than
runtime-discovered — simpler, deterministic, and Maxim already knows
both serials from SBFspot.

## Planned shape

### Config

```yaml
inverters:
  - slot: repeater
    bt_address: "00:80:25:21:32:35"
    # When `devices:` is present, this entry targets an MIS repeater.
    # The top-level password is used for MIS login-all (SBFspot
    # convention — devices can still have their own passwords if
    # non-default).
    password: "<shared or default>"
    devices:
      - slot: zolder
        app_serial: 2120121246
        model: "SB 3000HF-30"
      - slot: garage
        app_serial: 2120121383
        model: "SB 2000HF-30"
```

Backwards-compatible: if `devices:` is absent, legacy single-device
behavior (current behavior) kicks in.

### Session API

```rust
impl Session {
    pub async fn query_for_device(
        &mut self,
        susy_id: u16,
        serial: u32,
        kind: QueryKind,
    ) -> Result<Vec<u8>> { ... }
}
```

Body build: `build_query_body(kind, pkt_id, self.app_serial, susy_id, serial)`.
No handshake changes — the MIS login-all happens inside the existing
logon path (broadcast susy_id=0xFFFF, serial=0xFFFFFFFF, the inverter
replies once per device. We already receive multiple replies on the
logon broadcast — 0.1.21 notes "accept first retcode=0" — just need
to collect ALL of them instead of breaking on first.)

### Daemon loop

Per tick, iterate `inv_cfg.devices` and call `query_for_device` for
each. Publish to separate MQTT namespaces: `sbfspot_zolder_*`,
`sbfspot_garage_*`. Each device gets 28 sensors → 56 total for the
two-inverter setup.

Backoff + session-state stay at the repeater level. If the repeater
is down, everything's down. If one device stops responding while the
other keeps working, that's a per-device poll error — publish an
availability=offline on that device's LWT topic but keep the session
alive.

## Estimated work

- **Session changes**: ~50 LOC (query_for_device + logon broadcast
  collect-all + new state field `Vec<DeviceIdentity>`). 2 tests.
- **Config changes**: add `DeviceCfg` struct + `devices: Vec<DeviceCfg>`
  on `InverterCfg`. ~30 LOC + deserialize test.
- **Daemon changes**: outer tokio task iterates devices each tick. Per-
  device MQTT publisher instance. ~60 LOC + integration test.
- **MQTT discovery**: publish separate device + sensors per app_serial.
  ~20 LOC change to `mqtt-discovery::Publisher`.
- **Docs**: update COMPARISON.md MIS row, update DISCOVER.md shape B
  example from "future" to "supported".

Total: ~2 hours of focused work. Testable against the real MIS network
at any time the inverters are awake.

## Risks

- **Wrong password for garage** → retcode=0x0100 on garage queries only.
  Zolder unaffected. Log loudly, keep going. Provide per-device password
  override in config.
- **Repeater identity interferes with logon** — SBFspot observed that the
  repeater also replies to broadcasts. Filter to inverter-class replies
  only. Already partially handled by `recv_init_from_target` but that's
  BT-source-based; need a second filter for "is this actually an inverter
  or a communication product".
- **Per-device sleep times differ** — garage (smaller PV array) wakes
  later and sleeps earlier than zolder. Daemon should not treat one
  device's EHOSTDOWN-equivalent (retcode=0xFFFF from "device not
  responding") as "whole session asleep".

## What this is NOT

Not a runtime-discovery system. The user must configure serials
explicitly. Future phase: add a `probe` subcommand that runs a
"login-all" + device-status enumeration once and prints the config
YAML to stdout, so users don't have to hunt for serials.

## Status
Planned. No code yet. Current release (0.1.43) continues single-device
against zolder only. The discovery-mitigation is the planned behavior
documented in `docs/DISCOVER.md` — shape B is labeled as
"NOT supported yet".

## References

- Topology evidence: `memory/reference_sma_bt_topology.md` in the
  Claude Code HA project memory vault.
- Wire-level precedent: SBFspot's `IsMISEnabled()` branch in
  `SBFspot.cpp`.
- Existing single-device handshake path: `crates/inverter-client/src/session.rs`
  lines 299–560 (`handshake_and_logon`).
