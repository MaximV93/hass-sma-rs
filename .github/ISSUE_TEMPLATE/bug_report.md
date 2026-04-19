---
name: Bug report
about: Something broke — handshake, polling, MQTT, parser, you name it.
title: "[BUG] "
labels: bug
assignees: ''
---

## What happened

<!-- A clear description of what went wrong. -->

## What I expected

<!-- What you thought should happen instead. -->

## Environment

- hass-sma-rs version: <!-- from addon page OR `hass-sma-daemon --version` -->
- Home Assistant version: <!-- e.g. 2026.4.x -->
- Runtime: <!-- HA OS / Supervised / standalone Linux -->
- BT adapter: <!-- e.g. `hciconfig hci0` output — USB dongle? built-in Pi? -->
- Inverter model + firmware: <!-- e.g. SB 3000HF-30 02.30.06.R -->
- Topology: <!-- single BT / MIS repeater + N devices -->

## Logs

<!--
Paste the relevant log excerpt. Turn on DEBUG if you can:
  addon options → `RUST_LOG=debug`

Surround with triple backticks to format as code.
-->

```
```

## Captured bytes (strongly preferred for protocol bugs)

<!--
For parser/handshake bugs: capture the raw frames around the failure
and paste hex here. Lines like "body: 01 02 00 51 ..." from INFO logs
are ideal. The maintainer will add them to tests/fixtures/captured/
as a regression test.
-->

## Reproducibility

- [ ] Happens every poll
- [ ] Happens at specific times (dawn/dusk/etc)
- [ ] Happens after an addon restart
- [ ] Happened once and never again

## Anything else

<!-- Screenshots of HA, Grafana, etc. Any context that helps triage. -->
