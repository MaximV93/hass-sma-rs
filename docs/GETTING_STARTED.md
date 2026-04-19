# Getting started with hass-sma-rs

A 5-minute walkthrough from "never touched this addon" to "live SMA
data flowing into HA". Three scenarios, pick the one that matches your
install.

## Prerequisites (all scenarios)

- Home Assistant OS or Supervised 2024.x+.
- A Bluetooth-capable HA host. USB BT dongle works fine on Intel NUC
  / Proxmox LXC / bare-metal. Pi-built-in BT works too.
- The password for your inverter's **User** group (default `0000`
  unless someone changed it).
- The inverter's BT MAC address — printed on a sticker next to the
  serial number, or visible in the inverter's LCD menu under
  *System → Communication → Bluetooth Info*. Format: `00:80:25:XX:XX:XX`.

## Scenario A — single BT-direct inverter

The simplest case. One inverter, one BT radio.

### 1. Add the addon repo

Settings → Apps → ⋮ Repositories → Add
`https://github.com/MaximV93/hassio-addons`.

### 2. Install & configure

Install *hass-sma-rs (Rust rewrite)*, open the Configuration tab, and
use this YAML shape:

```yaml
mqtt:
  host: core-mosquitto
  port: 1883

inverters:
  - slot: zolder                  # your name for this inverter
    bt_address: "00:80:25:AA:BB:CC"
    password: "0000"              # user-group password
    poll_interval: 60s
    model: "SB 3000HF-30"         # optional, shows up in HA device card
    devices: []                   # required key, leave empty for shape A
```

Save and Start.

### 3. Verify

Within 10 seconds you should see:

- Log line `announced 29 sensors`
- 29 new entities in HA starting with `sensor.sbfspot_zolder_*`
- `sensor.sbfspot_zolder_poll_status` = `ok`
- `sensor.sbfspot_zolder_session_uptime` incrementing

That's the installation done.

## Scenario B — multiple inverters behind a BT repeater (MIS)

This is SMA's default when you have more than one inverter. A single
BT Repeater (or one inverter's built-in BT acting as master) fronts
2–7 inverters over an internal serial bus.

### 1. Find the repeater's MAC

If you already had haos-sbfspot or SBFspot running, grep its logs:

```
ssh hassio@<ha-host> "grep -i 'Connecting to' /share/sbfspot-logs/*.log | head"
```

The MAC it consistently connects to IS the repeater. If not, you can
discover it with the built-in probe subcommand:

### 2. Run the probe (safe, read-only)

First make sure no other SMA software is holding the BT adapter
(stop haos-sbfspot, close Sunny Explorer). Then from an HA SSH shell:

```bash
docker exec addon_e6b4b32d_hass-sma-rs \
  hass-sma-daemon probe --mac 00:80:25:AA:BB:CC --password 0000
```

You'll see:
- `BT piconet topology peers=[...]` — every BT-visible device
- `logon reply susy=... serial=... accepted=true/false` — every
  logged-in device
- A ready-to-paste YAML block with one `devices:` entry per serial

### 3. Configure MIS mode

```yaml
inverters:
  - slot: repeater              # just a label for logs + metrics
    bt_address: "00:80:25:AA:BB:CC"
    password: "0000"
    poll_interval: 60s
    devices:
      - slot: zolder
        app_serial: 2120121246
        model: "SB 3000HF-30"
      - slot: garage
        app_serial: 2120121383
        model: "SB 2000HF-30"
```

Each device gets its own HA device card with 29 sensors. Round-robin
polling over one RFCOMM session — no extra BT overhead.

## Scenario C — standalone Linux (no HA)

Build + run the daemon outside of HA:

```bash
git clone https://github.com/MaximV93/hass-sma-rs.git
cd hass-sma-rs
cargo build --release
sudo install -m 755 target/release/hass-sma-daemon /usr/local/bin/
sudo mkdir -p /etc/hass-sma-rs
sudo cp deploy/systemd/config.yaml.example /etc/hass-sma-rs/config.yaml
# edit /etc/hass-sma-rs/config.yaml
sudo install -m 644 deploy/systemd/hass-sma-rs.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now hass-sma-rs
```

The hardened systemd unit (PrivateUsers, SystemCallFilter, DeviceAllow)
is in `deploy/systemd/`. Full walkthrough: [deploy/systemd/README.md](../deploy/systemd/README.md).

## Recommended follow-ups

After installation, consider these optional next steps:

### Enable long-term archive
```yaml
archive:
  csv_dir: "/share/hass-sma-rs"     # zero-config, one CSV per day
  # OR
  timescale_url: "postgres://user:pass@host/db"
```

### Wire the Grafana dashboard
- Point Prometheus at `http://<ha-host>:9090/metrics`
- Import [docs/grafana-dashboard.json](grafana-dashboard.json)
- Import alerts from [docs/grafana-alerts.yaml](grafana-alerts.yaml)

### Add HA alerts
Dutch-bilingual health automations are shown in
[AUTOMATIONS.md](AUTOMATIONS.md). Covers:
- Stale-poll during daylight
- Consistent poll_status=error

## Troubleshooting

- **`Host is down` right after install** — inverter asleep. Arrives
  with sunrise. Adaptive backoff handles this automatically.
- **`Resource busy (EBUSY)`** — another addon or Sunny Explorer holds
  the BT adapter. Shut it down first.
- **`retcode=0x0100`** — wrong password. Double-check the User-group
  password; it's not necessarily the Installer password.
- **`retcode=0x0001` after addon restart** — inverter cached your
  previous session. 0.1.45+ persists `/data/app_serial_<slot>` to
  prevent this. If you're on an older version, wait ~15 min for the
  inverter's cache to expire.
- **MIS config but only one inverter shows up** — the probe
  subcommand only sees devices that answered logon; asleep devices
  don't reply. Retry at peak sun.

More: [docs/DISCOVER.md](DISCOVER.md), [docs/COMPARISON.md](COMPARISON.md).

## Where to get help

- Open a GitHub issue at <https://github.com/MaximV93/hass-sma-rs/issues>
- Read the session log of how this was built: [docs/SESSION_2026-04-19.md](SESSION_2026-04-19.md)
- Architecture decisions: [docs/adr/](adr/)
