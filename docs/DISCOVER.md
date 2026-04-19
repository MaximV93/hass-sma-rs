# Discovering additional inverters

How to find the BT MAC and user password for each SMA inverter on your
network so you can add them as a second/third `inverters:` entry in
hass-sma-rs config.

## 1. Let hass-sma-rs log the piconet for you

Since version 0.1.38 the daemon logs every BT device it sees on the
inverter's piconet during handshake. Look for lines like:

```
INFO: BT piconet topology peers=[
  "00:80:25:21:87:CA (inverter)",
  "00:80:25:1E:11:27 (inverter)",
  "00:80:25:21:32:35 (inverter)",
  "04:42:1A:5A:37:74 (host/local)"
]
```

`00:80:25:*` is SMA's Organizationally Unique Identifier — any BT MAC
starting with those three bytes is an SMA device. The "host/local" entry
is your Home Assistant / Raspberry Pi BT dongle itself.

Each `(inverter)` entry is a candidate for a new `inverters:` entry in
your config. Subtract the one you already have configured, and you're
left with candidates.

## 2. Map BT MAC → physical inverter

SMA prints the Bluetooth MAC on the inverter's nameplate sticker (the
small label usually beside the serial number and part of the 2-D
barcode). Easiest way:

- Climb to the inverter physically
- Read the `Bluetooth` field on the sticker
- That's the MAC to use

If the sticker isn't accessible, the inverter's own LCD knows:

- Press any button to wake the LCD
- Navigate: `System` → `Communication` → `Bluetooth Info`
- Note the MAC

## 3. Get the user password

SMA inverters default to `0000` for the User group. If someone has
changed it (the installer or a previous owner), you'll need to either
recover it via the inverter keypad reset procedure, or use the same
password you use for haos-sbfspot / Sunny Explorer (it's always the
same across all SMA interfaces on a given inverter).

## 4. Update the addon config

HA → Settings → Add-ons → hass-sma-rs → Configuration → edit YAML:

```yaml
inverters:
  - slot: zolder
    bt_address: "00:80:25:21:32:35"
    password: "<secret>"
    model: "SB 3000HF-30"
  - slot: zonneveld                        # NEW
    bt_address: "00:80:25:21:87:CA"        # NEW — from the topology log
    password: "<secret>"                   # NEW — same or different
    model: "SB 3000HF-30"                  # NEW — or whatever
    poll_interval: 60s
    mis_enabled: false
```

Restart the addon. Each inverter gets its own tokio task with
independent backoff, so if one goes to sleep at night the other keeps
running. Prometheus metrics and MQTT discovery sensors get a new set
automatically.

## 5. Verify

Logs should show one handshake + logon pair per inverter:

```
INFO inverter=zolder slot=zolder susy_id=131 serial=2120121246 — logged in
INFO inverter=zonneveld slot=zonneveld susy_id=131 serial=2120121383 — logged in
```

HA will create a second device card with 28 fresh sensors under the
new `sbfspot_zonneveld_*` namespace.

## Troubleshooting

- **`Host is down` on the new inverter's MAC** — night-time sleep.
  Wait for sunrise and retry.
- **`Resource busy`** — another addon or Sunny Explorer is holding the
  hci0 connection. Shut that process down first.
- **Handshake succeeds but logon retcode=0x0100** — wrong password for
  that inverter. SB inverters have per-device passwords; copy the
  value from Sunny Explorer / inverter LCD settings.
- **Handshake succeeds but queries return retcode=0xFFFF** — BT mesh
  confusion; the response is coming from a relay device not the target
  inverter. Check that the `bt_address` you configured actually matches
  what's on the nameplate; MIS networks have repeaters that also show
  up in the topology log.
