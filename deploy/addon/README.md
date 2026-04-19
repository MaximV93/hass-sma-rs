# Home Assistant addon wrapper

This directory is the HA-addon shape for hass-sma-rs: a tiny
Dockerfile + stub `config.yaml`. The authoritative addon manifest
(version, schema, options) lives in the companion repo
[MaximV93/hassio-addons/hass-sma-rs/](https://github.com/MaximV93/hassio-addons);
what's here is a reference copy the GitHub Actions pipeline uses to
build the runtime image.

## Files

```
deploy/addon/
├── Dockerfile     # musl-based Alpine image; compiles the daemon binary
└── config.yaml    # minimal wrapper; hassio-addons/config.yaml is the
                   # release-tagged version that users actually install.
```

## Build flow

1. Push to master or open PR → `.github/workflows/ci.yaml` runs fmt +
   clippy + test + cargo-audit + cargo-deny + release build +
   `docker build ghcr.io/<owner>/hass-sma-rs:<sha>`.
2. Push to the companion `hassio-addons` repo → its `builder.yaml`
   workflow detects `hass-sma-rs/config.yaml` changed, runs the
   home-assistant/builder action which:
   - Builds multi-arch (amd64 + aarch64)
   - Tags images `ghcr.io/<owner>/{arch}-addon-hass-sma-rs:<version>`
   - Pushes + signs via cosign
3. HA supervisor pulls the version-tagged image when the user hits
   "Update".

## Why two repos?

Maxim's home setup already had `hassio-addons` for the legacy
haos-sbfspot plus other HA addons. Adding hass-sma-rs there kept the
addon repo URL stable for existing users (same "Add repository" in
HA). The daemon source + CI live in this repo where they can evolve
independently of the addon's release cadence.

## Local docker run (for testing without HA)

```bash
docker build -f deploy/addon/Dockerfile -t hass-sma-rs:dev .
docker run --rm -it --network host \
  -v /data:/data \
  -e MQTT_HOST=core-mosquitto \
  hass-sma-rs:dev
```

Note: AF_BLUETOOTH + RFCOMM require `--network host` to access the
host's `hci0`. Docker-for-Mac / rootless Docker won't work; only
Linux with a real BT adapter.

For standalone-Linux (no HA at all), use the systemd unit instead —
see `../systemd/README.md`.
