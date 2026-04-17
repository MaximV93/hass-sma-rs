# Migration from bash fork to hass-sma-rs

The bash fork at `MaximV93/hassio-addons` tag `v-lastworking-2026.4.17.18`
stays in production while this daemon matures. Migration is staged in four
phases.

## Phase 1 — protocol validation (blocked on fixtures)

- Capture real BT traffic (see `FIXTURE_CAPTURE.md`).
- Drop fixtures into `tests/fixtures/`.
- Extend integration tests to parse every captured frame and assert against
  known-good SBFspot-side JSON outputs.
- Result: protocol crate is validated 1:1 against real inverters.

**Exit criterion:** 100% of captured `.hex` fixtures parse without errors.

## Phase 2 — parallel-run on a staging HA

- Install hass-sma-rs HA addon alongside the bash fork. Assign it an MQTT
  `state_prefix` of `hass-sma/` (ours), leaving the fork's
  `homeassistant/sbfspot_.../` alone.
- Wire both sets of sensors into a HA template sensor that computes the
  delta. Alert on drift > 1%.
- Run for one week (covers all weather + sleep/wake cycles).

**Exit criterion:** delta ≤ 1% for 7 consecutive days of daylight hours.

## Phase 3 — cutover weekend

- Stop bash fork addon.
- Flip HA dashboard + automations from `sensor.haos_sbfspot_*` / `sensor.sbfspot_*`
  to `sensor.sbfspot_<slot>_*` (hass-sma-rs naming).
- Migrate historical SpotData from MariaDB to TimescaleDB (script TBD).

**Exit criterion:** Energy Dashboard shows continuous history.

## Phase 4 — decommission bash fork

- Uninstall `e6b4b32d_haos-sbfspot` + tag v-lastworking in both repos.
- Archive MariaDB `sbfspot.SpotData` table (compressed dump to backup store).

## Rollback plan (any phase)

All state is namespaced so rollback to the bash fork is one addon restart
away. Backward-compat:

- `hass-sma-rs` uses `hass-sma/<slot>/<metric>` state topics — disjoint
  from the fork's `homeassistant/sbfspot_.../`.
- `hass-sma-rs` discovery configs live under
  `homeassistant/sensor/sbfspot_<slot>_<metric>/config` — same prefix but
  distinct object_ids; HA won't conflate.
- No changes to MariaDB schema in Phase 1 or 2. Phase 3 migration is a
  one-way operation that ships with a separate reversal script.

## Components after cutover

```
Inverters (2× SMA HF-30)
    │ BlueZ RFCOMM ch 1
    ▼
hass-sma-daemon (Rust, single binary)
    │
    ├─ MQTT publish → core-mosquitto
    │                       │
    │                       ▼
    │                 Home Assistant (entities + automations)
    │
    └─ TimescaleDB write → TimescaleDB (addon or external)
                               │
                               ▼
                         Grafana (optional)
```

Compared to the bash fork:

| Component | Fork | hass-sma-rs |
|---|---|---|
| Polling daemon | bash + cron + SBFspot C++ | Rust binary |
| BT session | open-close per poll | persistent |
| Protocol impl | 2010s-era C++ | 2026 Rust + tests |
| MQTT discovery | sensorsgen addon (separate) | owned by daemon |
| Time-series store | MariaDB SpotData | TimescaleDB (optional) |
| Observability | MQTT-as-metrics (indirect) | `/metrics` + structured logs |
| Container | Alpine + bashio + s6 (~250 MB) | distroless/cc (~25 MB) |
| Supply chain | Cosign, Trivy, SBOM (manual) | same, in CI by default |
| Test coverage | 4 smoke fixtures | 38 unit + integration tests |
