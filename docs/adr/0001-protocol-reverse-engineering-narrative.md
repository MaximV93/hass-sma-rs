# ADR 0001 — SMA BT protocol reverse-engineering narrative

Date: 2026-04-18
Status: accepted

## Context

SBFspot (the C++ reference implementation, GPLv2) has been the de-facto
way to read SMA Sunny Boy inverters over Bluetooth since 2011, but its
source does not document the wire protocol in a single place. Comments
assume the reader already knows SMA's quirks; the bash fork
(haos-sbfspot) wraps the binary without explaining what it does on the
wire. To reimplement this in Rust without a direct GPL dependency, I had
to rebuild the protocol understanding from (a) reading SBFspot source as
reference, and (b) capturing 290 real wire frames from a live SB
3000HF-30 inverter and diffing byte-for-byte against my implementation.

This ADR narrates the 12 protocol bugs found during live iteration so
future maintainers don't have to rediscover them.

## The wire, in three layers

**L0** — RFCOMM over AF_BLUETOOTH, channel 1. Standard BlueZ stack,
nothing SMA-specific except the channel number.

**L1** — SMA "frame" with:
```
0x7E | len_lo len_hi | hdr_cks | src_bt[6] | dst_bt[6] | ctrl[2] | payload | (optional FCS-16 LE | 0x7E trailer)
```
`len_lo len_hi` is little-endian total frame length INCLUDING the
leading 0x7E and (if present) the trailing 0x7E. `hdr_cks` is
`0x7E ^ len_lo ^ len_hi` — a trivial XOR check so the receiver can spot
a garbled header before consuming the body.

**L2** — SMA "packet" inside the L1 payload, keyed by a 4-byte signature
(`0xFF 0x03 0x60 0x65` little-endian):
```
signature(4) | longwords(1) | ctrl(1) | dst_susy_id(2) | dst_serial(4) |
ctrl2(2) | app_susy_id(2) | app_serial(4) | ctrl2_repeat(2) |
error_code(2) | fragment_id(2) | pkt_id(2) | command_body(N)
```
Wraps application-level commands (query, logon, etc.). The outermost L1
frame stuffs the L2 body with HDLC-style escape sequences
(`0x7E → 0x7D 0x5E`, same for 0x7D / 0x11 / 0x12 / 0x13).

## The twelve bugs

### 1. Framer must be length-driven, not delimiter-scanning

**Symptom**: `frame parse: declared length 31 != actual 32`
**Cause**: L1-only frames (hello, echo, ver_req) have NO trailing 0x7E.
A delimiter-scan reader waits for the next 0x7E as the frame terminator
— but that 0x7E is actually the START of the following frame.
**Fix**: parse the 4-byte header (0x7E + len + cks), verify the XOR
header checksum, then consume exactly `declared_len` bytes and emit.
Don't wait for a trailer.

### 2. L2 wire shape has a 0x7E separator byte

**Symptom**: inverter responses decode as "L1-only" with a payload that
starts with 0x7E followed by the L2 sig — `decode_l2` then fails because
it looks for the sig at payload[0..4].
**Cause**: SBFspot's `writePacket` writes a literal 0x7E byte between
the L1 header and the L2 signature (line 125 of SBFNet.cpp). This is
DIFFERENT from the HDLC escape — it's a raw unescaped 0x7E that the
receiver is expected to skip.
**Fix**: `Frame::parse` now detects L2 at offset 18 OR offset 19 (after
the 0x7E separator). `FrameBuilder::build_l2_wrapped` emits the
separator on send.

### 3. Retcode is in the L2 header, not the command body

**Symptom**: we kept getting `retcode=0x0201` when logon should return 0,
and trying to proceed optimistically caused queries to return
retcode=0xFFFF ("LRI not available").
**Cause**: SBFspot reads `get_short(pcktBuf + 23)` for the logon
retcode. That's L2 body offset 22..24 — the first "reserved" short in
what I initially assumed was just padding. SBFspot's ethPacket struct
calls it `ErrorCode` but the SBFNet.cpp comments don't explain it.
**Fix**: `L2Header::error_code` field at position [22..24], populated by
the sender (0) and overwritten by the inverter on reply.

### 4. pkt_id-matched reply dispatch

**Symptom**: logon reply body contained the init reply's data,
SpotAcTotalPower contained logon echo, etc. Stale replies from prior
requests were still in the RFCOMM buffer when we sent the next request.
**Cause**: SBFspot's `getPacket` explicitly loops while
`pcktID != rcvpcktID`; we were just reading the next frame blindly.
**Fix**: `Session::recv_l2_with_pkt_id()` loops up to 16 frames
discarding anything with a non-matching `pkt_id`.

### 5. Full SBFspot handshake sequence

**Symptom**: logon consistently rejected with retcode=0x0001 even with
byte-correct bytes.
**Cause**: we were doing `recv_hello → send_ver → send_logon`.
SBFspot's `initialiseSMAConnection` does MUCH more:
```
recv hello → send echo(NetID) → recv topology → send L2 init (cmd 0x0200)
  → recv init reply (learns inverter SUSyID + serial) → send logoff
  → 300ms wait → send logon → recv logon reply
```
The inverter's state machine expects every step. Skipping any produces
0x0001.
**Fix**: rewrote `handshake_and_logon` to match line-for-line.

### 6. Stable app_serial across reconnects

**Symptom**: first session logged in fine, every reconnect got
retcode=0x0001. Waiting 15 min (nominal session timeout) didn't help.
**Cause**: each `Session::new()` generated a fresh random `app_serial`
via `session_id()`. The inverter tracks clients by app_serial and treats
a reconnect with a new one as a session-conflict attempt, not a
continuation.
**Fix**: daemon generates ONE `app_serial` per inverter at startup
(salted by slot name for multi-inverter uniqueness) and passes it into
`Session::new_with_app_serial()` on every reconnect. The inverter now
recognises us as the same client re-establishing.

### 7. Target-BT init filter for MIS networks

**Symptom**: every query came back from a susy=102 / serial=170004597
device that wasn't zolder. Query returned retcode=0xFFFF for every LRI.
**Cause**: in a multi-inverter BT mesh, the L2 `init` broadcast reaches
every device. Each replies with its own susy/serial. We took the first
reply — which was from a relay/peer, not zolder. We then stored its
identity and addressed queries to it.
**Fix**: `recv_init_from_target()` loops init replies and only accepts
one whose L1 source BT matches our RFCOMM peer (the inverter we
physically connected to).

### 8. Unsolicited ctrl=0x0008 push frames

**Symptom**: SpotAcPower query timed out despite the inverter clearly
having sent data (visible in hex dump).
**Cause**: the inverter sometimes responds with `ctrl=0x0008` (unsolicited
data push) instead of `ctrl=0x0001` (classic reply). The L2 body is the
same shape, pkt_id matches, but we were filtering on `ctrl=0x0001`.
**Fix**: `recv_l2_with_pkt_id` now accepts ANY L1 ctrl as long as the
inner L2 parses and the pkt_id matches.

### 9. Push frames have no trailing 0x7E and no FCS

**Symptom**: ctrl=0x0008 frames passed the ctrl-agnostic filter but then
failed `Frame::parse` with "missing end delimiter 0x7E (found 0x80)".
**Cause**: push frames don't have the classic FCS-16 + 0x7E trailer. The
body runs to the end of the declared length.
**Fix**: `Frame::parse` branches on `raw.last() == 0x7E`:
- trailer present → strip 2-byte FCS + trailer from unstuffed body
- no trailer → take the whole unstuffed body verbatim

### 10. SoftwareVersion is BCD at record offset 24

**Symptom**: firmware version displayed as `255.255.254.?` (garbage).
**Cause**: I read the packed u32 at record offset 16 (where spot values
live) and decoded it as plain decimal. SBFspot reads `get_long(recptr + 24)`
and the bytes are `[vtype, vbuild, vminor, vmajor]` where vmajor/vminor
are BCD (0x30 → "30", not 48).
**Fix**: read from [24..28], unpack BCD for major/minor, treat vbuild
as plain decimal, map vtype through "NEABRS" table.

### 11. LRI extraction uses `& 0x00FF_FF00`, not `& 0x00FF_FFFF`

**Symptom**: random LRI mismatches, some records silently skipped.
**Cause**: records encode `code = dataType << 24 | LRI_16 << 8 | cls`.
Masking `code & 0x00FFFFFF` gave us `LRI_16 << 8 | cls` — the cls byte
polluted what should be the LRI key.
**Fix**: mask `code & 0x00FF_FF00` to isolate the 16-bit LRI in the
middle two bytes. Use the low byte separately as `cls` (channel id) for
per-phase / per-string dispatch.

### 12. Spot value at record offset 16, short records are valid

**Symptom**: all spot values (W, V, A, Hz, °C) showed as min/max NaN
sentinels or random zeros.
**Cause**: I read `value_i32` at offset [8..12]. That slot carries the
record's MIN value, not the current reading. SBFspot reads
`get_long(recptr + 16)`. AND some records are only 20 bytes (truncated)
when trailing min/max/flag fields would all be NaN.
**Fix**: read value at [16..20], 0x80000000 → None. Record iteration
accepts MIN=20..MAX=28 stride per-record.

### Bonus: TypeLabel is 40-byte records with 8 attribute slots

Not on the original list of 12, but came up at the end:

**Symptom**: Inverter Model sensor showed "TagID 842084913".
**Cause**: TypeLabel records are 40 bytes (not 28) and contain EIGHT
attribute slots at offsets 8/12/.../36. Each is a u32 where
`attr >> 24 == 1` marks selected, `0xFFFFFE` is end-of-list, and
`attr & 0x00FFFFFF` is the tag id. SBFspot's `getattribute()` iterates
all eight.
**Fix**: dedicated 40-byte record loop for `parse_type_label_raw`.

## Bugs added after original narrative

### 14. Yield-window teardown needs LOGOFF + post-yield grace (2026-04-19)
Two compounding issues observed live at 07:54 during the first sunrise
with parallel-run yield enabled:

- Dropping the RFCOMM socket without LOGOFF left a zombie session on
  the inverter for ~15 min. Post-yield reconnects hit EHOSTDOWN.
  Fixed in 0.1.42 by `Session::graceful_close()` sending a proper
  LOGOFF (broadcast dst, L1 ctrl=0x0001, 150ms grace).
- Even with clean LOGOFF, the inverter's BT radio can take up to a
  minute to re-advertise. The adaptive-backoff code escalated to a
  10-minute "asleep" state after 3 fast EHOSTDOWNs. Fixed in 0.1.43
  with a 180-second post-yield grace window that uses 15s retries
  without incrementing the sleep-streak.

## Topology addendum (2026-04-19)

The original narrative assumed single-inverter direct-BT installs.
Real-world MIS topology: one BT repeater fronts N inverters on an
internal bus. The MAC on the inverter nameplate sticker is often
actually the repeater's radio. At the protocol layer each L2 request
addresses a specific `(susy_id, serial)` in the header — the repeater
routes, no handshake changes needed for multi-device polling.
See [ADR 0005](0005-mis-multi-inverter.md).

Implication for this narrative: every bug here was found BEHIND a
repeater. The protocol is wire-identical whether you have one
inverter or ten. Only the daemon's poll loop needed to grow a
round-robin over configured devices.

## Consequences

Every bug above has a regression test in the Rust codebase using either
a synthetic fixture or a real-capture hex string. The 290-frame fixture
suite prevents the classic "fix one bug, introduce another" cycle.

Anyone porting this protocol (to another language, another transport) can
use this ADR as a checklist. The order here matches the order we hit
them during live iteration — if you follow it, each fix unlocks the
next bug (you can't test #3 until #2 is right, etc.).

## References

- SBFspot source: <https://github.com/SBFspot/SBFspot>
  (license CC BY-NC-SA for reverse-engineering; all source is clean-room)
- RFC 1662 §C.2 FCS-16 — our PPP-compatible implementation
- `tests/fixtures/captured/*.hex` — 290 wire frames classified into
  L1-valid / L2-only-blob / truncated buckets
- `crates/sma-bt-protocol/tests/captured_frames.rs` — roundtrip tests
