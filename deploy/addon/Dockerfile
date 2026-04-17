# ───── build stage ─────────────────────────────────────────────────────────
# Use rustlang/rust:nightly to stay ahead of stable's MSRV gate (our workspace
# pins rust 1.75 min but some transitive deps need a recent compiler).
FROM rust:1-slim-bookworm AS builder
WORKDIR /build

COPY Cargo.toml ./
COPY crates/ crates/
RUN cargo build --release --bin hass-sma-daemon

# ───── runtime stage ──────────────────────────────────────────────────────
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
COPY --from=builder /build/target/release/hass-sma-daemon /usr/local/bin/hass-sma-daemon

# Default config path; override with `--config` on the command line.
ENTRYPOINT ["/usr/local/bin/hass-sma-daemon"]
CMD ["--config", "/data/options.yaml"]
