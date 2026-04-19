# Security policy

## Supported versions

hass-sma-rs pre-1.0 is rapid iteration — only the latest release line
receives security fixes. That's currently the 0.1.x train; tip of
master is generally what ships next.

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅ |
| < 0.1   | ❌ (never released publicly) |

## Reporting a vulnerability

Please do NOT open a public GitHub issue for security-sensitive
reports. Instead:

- **Preferred:** use GitHub's private security advisory form at
  <https://github.com/MaximV93/hass-sma-rs/security/advisories/new>.
  Credentials for the maintainer arrive directly.
- **Alternative:** email the maintainer — `maxim.vandebroeck@gmail.com`.

Please include:
- Affected version / commit hash.
- Minimal reproducer or PoC (pseudocode is fine if executing is risky).
- Impact assessment (what an attacker gains, prerequisites).
- Your preferred credit wording (or "anonymous").

## What we consider in-scope

- **Protocol-parse panics or memory-safety violations** in the
  `sma-bt-protocol`, `bluez-transport`, or `inverter-client` crates.
  The parser MUST never panic on adversarial bytes; the existing fuzz
  suite in `crates/sma-bt-protocol/tests/fuzz_frame_parse.rs`
  enforces this, but finds count.
- **Auth bypass or session takeover** — e.g., ways to accept data
  from a BT peer other than the configured inverter.
- **Secrets leakage** — passwords, LWT tokens, or app_serial ids
  appearing in logs / metrics / MQTT at levels a typical user would
  retain.
- **Privilege escalation** in the HA addon — anything that lets the
  container escape its declared capabilities (AF_BLUETOOTH only).

## What we consider out-of-scope

- The SMA protocol itself is inherently insecure (plaintext
  password, no session encryption). hass-sma-rs doesn't invent
  cryptography the underlying protocol lacks.
- Denial of service via flooding the addon with garbage frames —
  the Parser refuses malformed frames, but if you can control the
  BT radio of a trusted peer, you can always make it go silent.
- Third-party dependencies — report upstream (we track advisories
  via cargo-audit + deny.toml, see `/deny.toml`).

## Response timeline

- **24h** — acknowledgement that we received the report.
- **7 days** — triage + confirmed severity + ETA for fix.
- **≤ 30 days** — fix landed for HIGH / CRITICAL. LOW / INFORMATIONAL
  goes to the next feature release.

## Disclosure

Coordinated. We'll publish a security advisory + CHANGELOG entry
simultaneously with the fix release. Credit goes to the reporter by
default (opt-out available).
