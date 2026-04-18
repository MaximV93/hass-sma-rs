# systemd deployment (non-HA hosts)

For running hass-sma-rs on a standalone Linux box (Raspberry Pi, x86
Linux server, etc.) without the HA addon container.

## Install

```bash
# Create system user
sudo useradd --system --no-create-home --shell /usr/sbin/nologin hassma

# Directories
sudo mkdir -p /etc/hass-sma-rs /var/lib/hass-sma-rs
sudo chown hassma:hassma /var/lib/hass-sma-rs

# Binary (cargo build --release produces target/release/hass-sma-daemon)
sudo install -m 755 target/release/hass-sma-daemon /usr/local/bin/

# Config (adapt the example below)
sudo cp deploy/systemd/config.yaml.example /etc/hass-sma-rs/config.yaml
sudo chown root:hassma /etc/hass-sma-rs/config.yaml
sudo chmod 640 /etc/hass-sma-rs/config.yaml

# systemd unit
sudo install -m 644 deploy/systemd/hass-sma-rs.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now hass-sma-rs
```

## Verify

```bash
sudo systemctl status hass-sma-rs
sudo journalctl -u hass-sma-rs -f
curl http://localhost:9090/metrics | head
```

## Hardening notes

The unit enables most of systemd.exec(5)'s sandboxing:

- `NoNewPrivileges` — no setuid binary escapes
- `ProtectSystem=strict` — /usr and /etc read-only
- `ProtectKernelTunables/Modules/Logs` — no kernel surface access
- `PrivateUsers` — user namespace isolation
- `DevicePolicy=closed` + `DeviceAllow=/dev/rfcomm0 rw` — only BT RFCOMM
- `RestrictAddressFamilies` — AF_BLUETOOTH + AF_INET[6] + AF_UNIX only
- `SystemCallFilter=@system-service` — strip privileged/debug syscalls
- `MemoryMax=128M` — cgroup memory cap (daemon typically uses <20 MB)
- `CapabilityBoundingSet=CAP_NET_RAW CAP_NET_ADMIN` — nothing else

If your BlueZ setup routes RFCOMM differently (no `/dev/rfcomm0` node),
remove the `DeviceAllow` line and set `PrivateDevices=true` — the
AF_BLUETOOTH socket path doesn't need a device node.

## Why a non-addon deployment?

Useful scenarios:

- **Dedicated BT gateway** — a Raspberry Pi Zero 2 W sitting physically
  close to the inverter, forwarding data over WiFi to an HA instance
  elsewhere in the house. ~€20 hardware, solves weak BT signal at HA.
- **Parallel-run migration** — run hass-sma-rs on a separate machine
  while haos-sbfspot owns hci0 on the HA host.
- **Non-HA environments** — Proxmox LXC, Docker Compose, bare-metal
  homelabs where you want the Prometheus `/metrics` endpoint but not
  HA's MQTT discovery.

## Example config

See `deploy/systemd/config.yaml.example` for a minimal working config.
