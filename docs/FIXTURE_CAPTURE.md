# Capturing SMA BT fixtures

To iterate on the protocol parser with confidence we need **real** bytes off
the wire. The integration tests in `crates/inverter-client/tests/` currently
use fabricated responses that satisfy the shape the protocol crate expects —
but only field captures can catch every corner of what an HF-30 actually
emits (endianness, ordering, optional records, sleep-state responses, …).

## What to capture

For each inverter, during daytime:

1. A full **handshake sequence** — from RFCOMM connect through first query.
2. **Every `QueryKind`** listed in `crates/sma-bt-protocol/src/commands.rs`,
   both with the inverter awake and with it at night (asleep).
3. **Logon failure** with a deliberately wrong password.
4. **BT drop** mid-session (pull the repeater power, resume, capture the
   error frames).

## How to capture (Linux box with BlueZ)

The cleanest method is a `btmon` + SBFspot pair, filtering on the RFCOMM
session between our local adapter and the inverter MAC.

```bash
sudo btmon -w /tmp/sma-<slot>-<scenario>.btsnoop &
BTMON_PID=$!

# Run SBFspot once end-to-end (cron-mode is simplest):
/usr/bin/sbfspot/SBFspot -v -ad1 -am0 -ae0 -mqtt

kill $BTMON_PID
```

Name each file: `sma-<slot>-<scenario>-<date>.btsnoop` (e.g.
`sma-zolder-handshake-2026-04-17.btsnoop`).

## Extracting SMA frames from a btsnoop

```bash
# btsnoop → plaintext, filtering RFCOMM L2CAP packets
btmon -r /tmp/sma-zolder-handshake-2026-04-17.btsnoop > /tmp/dump.txt

# Strip to bare hex payloads (one line per frame, starting with `7e`):
python3 scripts/btsnoop-to-hex.py /tmp/dump.txt > /tmp/frames.hex
```

(Script to be written once we have a real capture.)

## Dropping fixtures into the repo

```
tests/fixtures/
  handshake/
    hello.hex
    ver-reply.hex
    logon-ok.hex
  query/
    spot_ac_total_power.hex
    energy_production.hex
    software_version.hex
  failure/
    logon-invalid-password.hex
```

Each `.hex` file is one frame per line. The test harness will:

1. Feed the hex through `hex::decode` → `FrameReader::push`.
2. Assert `Frame::parse` succeeds and produces the expected `L2Header`.
3. For queries, assert `parse_spot_ac_total_power` (et al.) returns sane values.

## Why this matters

Today we're blocked on the **assumption** that our protocol crate produces
the exact byte layout an inverter expects. Property-based tests and FCS
reference vectors cover the general-case math. Real fixtures cover
SMA-specific idiosyncrasies (e.g. the extra L1 header bytes SB-SMA inserts
under MIS mode, the "0x1001 then 0x0005" two-step packet sequence during
topology discovery, and the LRI records that vary per-firmware).

Without these fixtures we can reach **tested-scaffold-quality**. With them,
we reach **production-ready** for Maxim's specific two-inverter setup.
