---
name: Feature request
about: Something you want hass-sma-rs to do that it doesn't today.
title: "[FEATURE] "
labels: enhancement
assignees: ''
---

## What problem are you trying to solve?

<!--
Start with the USE CASE, not the solution. "I want X so I can Y" is
much more useful than "add API Z" — the maintainer may know a better
Z that already works.
-->

## Proposed solution

<!-- Your idea for how to solve it. -->

## Alternatives considered

<!-- Other approaches you thought about. -->

## Scope signals

- [ ] Affects wire protocol (SMA BT bytes) — high bar, need captured evidence
- [ ] Affects MQTT / HA discovery shape — medium, careful about breaking existing sensor IDs
- [ ] Affects daemon config surface — medium, schema is supervised-validated
- [ ] Docs / CI / tooling — low bar

## If you're willing to implement

- [ ] I'll send a PR once the design is agreed.
- [ ] I can test against my hardware (tell us the model).
- [ ] Not myself, but happy to debug + review.
