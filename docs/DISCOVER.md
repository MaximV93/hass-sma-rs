# Discovering additional inverters on your SMA piconet

> **Updated 2026-04-19**: the previous version of this document assumed
> a "2 BT direct inverters" topology. For MIS networks (repeater +
> multiple inverters behind it) that shape is WRONG — see below.

## Two network shapes

### Shape A: two direct BT inverters

```
hci0 → inverter1 (BT MAC A)
     → inverter2 (BT MAC B)
```

Simplest case. Add a second `inverters:` entry in the addon YAML with the
second inverter's own BT MAC. Each gets its own tokio task + RFCOMM session.

### Shape B: MIS network (BT repeater + N inverters)

```
hci0 → BT Repeater (one MAC, e.g. 00:80:25:21:32:35)
         ├── inverter1   (susy_id X, serial Y1)
         ├── inverter2   (susy_id X, serial Y2)
         └── inverter3   (susy_id X, serial Y3)
```

This is SMA's default on any multi-inverter installation. The repeater
advertises on BT; the inverters are reached via the repeater using
app_serial addressing in the L2 header of each request.

**hass-sma-rs does NOT yet support Shape B multi-inverter polling.**
Currently only the first inverter identified by the init-reply is
queried. Multi-device MIS support is on the roadmap; the protocol is
already per-request-addressed so it's implementable without a
handshake redesign.

Tracking: [#TBD](https://github.com/MaximV93/hass-sma-rs/issues)

## Recognising which shape you have

### 1. Read the topology log

Since version 0.1.38 the daemon logs every BT device in the piconet
during handshake. Look for:

```
INFO: BT piconet topology peers=[
  "00:80:25:21:32:35 (inverter)",
  "04:42:1A:5A:37:74 (host/local)"
]
```

- **One `(inverter)` entry** → either shape A single-inverter, or
  shape B with the repeater counted as a single BT device. Check the
  device's identification (via SBFspot's `SBFspotUploadDaemon` or by
  connecting with Sunny Explorer) — if `DeviceClass = "Communication
  products"` and `DeviceType = "Bluetooth Repeater"`, you're in
  shape B.
- **Multiple `(inverter)` entries** → shape A, multi-direct-BT.

### 2. Check SBFspot's legacy logs if you had that addon

If haos-sbfspot ran previously, its logs reveal the real topology.
Look for:

```
sudo grep -E 'Device Type|Connecting to' /share/sbfspot-logs/*.log
```

Multiple `InvType` values behind one `Connecting to <MAC>` = shape B.

## Configuring a second inverter

### Shape A (only)

```yaml
inverters:
  - slot: zolder
    bt_address: "00:80:25:AA:BB:CC"
    password: "<secret>"
    model: "SB 3000HF-30"
  - slot: garage
    bt_address: "00:80:25:DD:EE:FF"       # DIFFERENT MAC — must be direct-BT
    password: "<secret>"
    model: "SB 2000HF-30"
```

If the two entries' `bt_address` happen to collide (same MAC), the
second task will EBUSY on RFCOMM and log-retry indefinitely. That's
the signal you're actually in shape B.

### Shape B (future)

Single `inverters:` entry for the BT repeater MAC plus a `devices:`
sub-list with app_serial + password per device. Not wired yet — when
implemented, config will look like:

```yaml
inverters:
  - slot: repeater
    bt_address: "00:80:25:21:32:35"
    devices:
      - slot: zolder
        app_serial: 2120121246
        password: "<secret>"
        model: "SB 3000HF-30"
      - slot: garage
        app_serial: 2120121383
        password: "<secret>"
        model: "SB 2000HF-30"
```

Each device gets its own HA sensors + MQTT topic space; all share the
single RFCOMM socket.

## Troubleshooting

- **`Host is down` on a supposedly-direct second inverter** — likely
  you're in shape B and the second MAC doesn't actually exist. Double
  check via the topology log.
- **`Resource busy (EBUSY)`** — another addon or Sunny Explorer holds
  the hci0 connection. Shut that process down first.
- **Handshake succeeds but logon retcode=0x0100** — wrong password for
  that serial. SMA inverters have per-device passwords; copy the
  value from Sunny Explorer or the inverter's LCD settings.
- **Handshake succeeds but queries return retcode=0xFFFF** — addressed
  a device that doesn't exist or is asleep. In shape B, the repeater
  replies retcode=0xFFFF for unknown app_serial.
