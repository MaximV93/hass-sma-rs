# ───── build stage ─────────────────────────────────────────────────────────
FROM rust:1.81-slim-bookworm AS builder
WORKDIR /build

# Cache deps via a dummy build first.
COPY Cargo.toml ./
COPY crates/ crates/
RUN cargo build --release --bin hass-sma-daemon

# ───── runtime stage ──────────────────────────────────────────────────────
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
COPY --from=builder /build/target/release/hass-sma-daemon /usr/local/bin/hass-sma-daemon

# Default config path; override with `--config` on the command line.
ENTRYPOINT ["/usr/local/bin/hass-sma-daemon"]
CMD ["--config", "/data/options.yaml"]
