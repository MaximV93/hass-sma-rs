# ADR 0003 — Rust over C++ for the rewrite

Date: 2026-04-18
Status: accepted

## Context

Stock SBFspot is ~12.5 kLOC of C++11 (2011 codebase with incremental
additions). The bash fork (haos-sbfspot) wraps it. A rewrite could
stay in C++ (modernize the existing codebase), move to Go, or move to
Rust. I considered each.

## Decision

Rust.

## Rationale

### Memory safety where it actually matters

The SMA BT protocol is dense with `get_long(pcktBuf + N)` style
pointer arithmetic. Every offset in SBFspot is open-coded. The
original codebase has at least one historical CVE pattern
(unbounded memcpy into `pcktBuf`) and multiple `int` / `uint8_t`
narrowing conversions that silently mask high-order bytes.

In Rust's type system these become either compile errors or explicit
casts. The 290-frame fuzz-style fixture test in
`captured_frames.rs` catches any miss; the `Frame::parse` branches
are exhaustive by type.

**Actual benefit this session**: three of the twelve bugs (#1 framer
length, #5 handshake steps, #12 short records) would have been much
harder to diagnose with C++'s implicit conversions and array-
pointer decay. The Rust compiler's "you haven't handled these record
sizes" nag forced me to think about the variable-stride problem.

### async/await makes the per-inverter multitasking trivial

```rust
for inv in cfg.inverters {
    tasks.push(tokio::spawn(async move {
        run_inverter(inv, ...).await
    }));
}
```

That's a complete multi-inverter scheduler. In C++ this would require
either threads (with all the blocking-RFCOMM headaches) or an event
loop framework (libevent, asio). In Go it'd be goroutines, which is
fine but Go's struct packing is looser and the stdlib doesn't have
as good a Prometheus client.

### Ecosystem fit

- `rumqttc` — drop-in async MQTT client with LWT support
- `prometheus-client` — proper Prometheus text exposition format
- `tracing` — structured logs with JSON output, trivial to ship to Loki
- `serde_yaml` — config parsing
- `tokio` — async runtime
- `byteorder` — explicit little/big-endian reads (no "which way did I
  byteswap this" confusion from cultural C preferences)
- `thiserror` — ergonomic error types

Every piece of infrastructure we'd build anyway already exists,
well-maintained, zero-warning with `clippy -D warnings`.

### Binary size / deployment

A release-mode build on musl Alpine weighs ~8 MB. HA addon
container cold-start is under 2 seconds. No Python interpreter,
no JVM, no Node runtime. Fits in a distroless or scratch base.

### Type-driven protocol decoder

L2Header, Frame, Session::State, ParseError — all `enum`s or
structs with explicit fields. Reviewers can read the struct def
and know exactly what's in the reply. Field name drift (SBFspot
has `ctrl`, `ctrl2`, `ctrl2_repeat`, `Ctrl`, `CtrlRec` used
interchangeably) is eliminated by having ONE source of truth in
`crates/sma-bt-protocol/src/packet.rs`.

## Consequences

### Good

- 54 tests with fixture-based regression for every bug found
- `cargo clippy -D warnings` passes = forced handling of every match arm
- Full async multi-inverter with 50 lines of wrapper code
- Fast compile (warm: <5s; cold: ~30s)
- Distroless deploy target trivial

### Bad

- Smaller talent pool than "add it to SBFspot upstream". But this is
  a private fork.
- Can't directly pull fixes from SBFspot upstream — need to re-port
  manually. Mitigated because the protocol is stable (SMA hasn't
  shipped new BT firmware for SB HF/TL in years).
- Compile times scale with dep tree. We have ~80 transitive deps;
  clean build is ~40s. Could trim if ever needed.

### Neutral

- Error handling is more verbose than C++ exceptions. I think it's
  worth it — every protocol error we might hit is explicitly
  `SessionError::*` rather than a stringly-typed exception bubbling
  up from who-knows-where.

## Alternatives considered

**C++20 modernization of SBFspot**. Would keep some of the pointer
arithmetic muscle memory useful. Rejected because (a) we'd still own
the old bugs, (b) the CMake + boost + OS detection in the existing
codebase is more work to clean up than to bypass.

**Go**. Strong ecosystem, easy multi-task via goroutines. Rejected
because Prometheus/MQTT/tracing lib fit is worse and Rust's
type system catches more protocol bugs at compile time (the LRI mask
bug would have compiled in Go because `uint32 & 0xFFFFFFFF` is just
a no-op; Rust made me write the mask constant where it couldn't
implicitly widen).

**Python**. Rejected because of packaging overhead (the HA addon
would ship a full Python interpreter) and the async ecosystem is
weaker for BT-adjacent tasks.

## References

- `Cargo.toml` / workspace structure
- ADR 0002 — persistent session model relies on async
