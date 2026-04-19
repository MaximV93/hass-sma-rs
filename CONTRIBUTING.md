# Contributing to hass-sma-rs

Thanks for considering a contribution. This project is single-author at
the time of writing and happy to take patches.

## Dev setup

### Prerequisites

- Rust stable **1.75+** (specified in `Cargo.toml` `rust-version`).
  Clippy + rustfmt must come from the same channel.
- Linux with `libbluetooth-dev` (Debian/Ubuntu) or equivalent if you
  want to touch `bluez-transport`'s real BT code. Protocol + parser
  crates build anywhere.
- Docker (optional) if you want to reproduce the addon image build.

### Clone + build

```bash
git clone https://github.com/MaximV93/hass-sma-rs.git
cd hass-sma-rs
cargo build --workspace
```

### Run the test suite

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

A green CI run on every PR requires all three. Plus `cargo-deny check`
(advisories + bans + licenses + sources) and `cargo-audit` — both
advisory-only but the goal is green.

## Code style

- **rustfmt is authoritative.** Run `cargo fmt --all` before committing.
  Config lives in `rustfmt.toml` if one exists — today we use the
  default channel formatter.
- **100-col soft limit** per rustfmt. Break lines that would wrap.
- **Clippy `-D warnings`** in CI — no suppressions without an inline
  justification comment.
- **No `unwrap()` on fallible I/O** in production code paths. Tests may
  unwrap freely.
- **No panics in library crates** (`sma-bt-protocol`, `bluez-transport`,
  `inverter-client`, `mqtt-discovery`, `telemetry`, `storage`). Return
  `Result`. The daemon binary is allowed panics only in `main()` setup.
- **`tracing` for logs**, not `println!` or `eprintln!`. Pick the level
  carefully: INFO for session lifecycle, WARN for recoverable, ERROR
  for "we had to tear down".

## Commit conventions

- **One logical change per commit.** Don't mix refactors with feature
  work.
- **Subject line ≤ 72 chars, imperative mood.** "Add event log parser"
  not "Added event log parser".
- **Body explains WHY.** Reviewers don't need to be told WHAT
  (diff speaks); they need to know what problem this solves.
- **Link the ADR** if your change affects architecture: `See ADR 0005`.
- **No AI-attribution lines.** The maintainer doesn't want them in git
  history — see project CLAUDE.md for rationale.

### Version bumps + CHANGELOG

Every user-visible change updates `CHANGELOG.md`. The top entry template:

```md
## 0.1.X — YYYY-MM-DD (one-line summary)

### Added / Fixed / Changed / Deprecated / Removed
- one concrete bullet per change (not "misc cleanup")
```

For addon releases, also bump `config.yaml` `version:` in the companion
repo `MaximV93/hassio-addons`.

## Testing philosophy

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) § "Testing philosophy"
for the overall approach. TL;DR:

1. **Protocol-layer code** must have unit tests using captured bytes,
   not hand-invented ones where possible.
2. **Parser changes** → add a test that uses the EXACT bytes the bug
   was observed on. We keep captures under `tests/fixtures/captured/`.
3. **Session-layer changes** → MockTransport test. Don't require real
   hardware for every PR.
4. **Proptest** is available (`proptest = "1"`) if your change has a
   clear invariant (never-panic, roundtrip, etc.) — prefer it over
   hand-crafted example-based tests for parser work.

## Reverse-engineering a new protocol feature

Process that worked for the 14 protocol bugs fixed this April:

1. Capture the traffic. `bluetoothctl` + a logic analyzer or
   `btmon -w capture.btsnoop` works. Commit the raw bytes as a fixture.
2. Decode by hand against SBFspot's C++ reference (`SBFspot/SBFspot`
   on GitHub — CC BY-NC-SA, reference only, no copy-paste).
3. Write a failing test using the captured bytes.
4. Implement until green. Add to `CHANGELOG.md` + relevant ADR.

Don't skip step 1. Captured bytes catch subtle wire-shape surprises
(ctrl=0xE0 vs 0xA0, byte-stuffing corner cases, value offsets) that
spec-reading alone misses.

## Pull-request workflow

1. Fork → feature branch → commits → open PR against `master`.
2. CI runs: fmt, clippy, test, cargo-audit, cargo-deny, release build,
   docker build. See `.github/workflows/ci.yaml`.
3. Addon changes trigger a separate Builder run in `MaximV93/hassio-addons`
   when `hass-sma-rs/config.yaml` is touched there.
4. Merge squashes are OK if your PR has one logical change; otherwise
   rebase-merge to preserve the commit story.

## Reporting bugs + asking for features

Open an issue at <https://github.com/MaximV93/hass-sma-rs/issues> with:

- The hass-sma-rs version you're running (from addon page or
  `hass-sma-daemon --version`).
- Relevant log excerpt. Use `tracing::Level::DEBUG` if you can — INFO
  alone is often not enough for protocol bugs.
- Your inverter model + firmware. Helps match against captured
  fixtures.

For a **protocol bug**, a 10-line raw-byte capture around the failure
point is worth a thousand words — include if possible.

## License

Rust code: **MIT OR Apache-2.0** at your option. Contributions are
assumed to be under the same dual license unless you explicitly say
otherwise in the PR description.

Protocol reference from SBFspot is **CC BY-NC-SA** — read but don't
copy. Captured frames in `tests/fixtures/captured/` are from the
maintainer's own inverters and are distributed here under Apache-2.0.
