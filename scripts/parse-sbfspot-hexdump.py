#!/usr/bin/env python3
"""Parse SBFspot's `-d5` hex-dump output into per-frame fixtures.

SBFspot's HexDump() format:

    --------: 00 01 02 03 04 05 06 07 08 09
    00000000: 7E 17 00 69 00 00 00 00 00 00
    00000010: 01 00 00 00 00 00 01 02 76 65
    00000020: 72 0D 0A
    23 Bytes sent

Received packets are framed by:

    <<<====== Content of pcktBuf =======>>>
    --------: 00 01 02 03 04 05 06 07 08 09
    00000000: 7E 1F 00 61 ...

Every frame is emitted here — both sent and received. The trailing byte
count ("23 Bytes sent") distinguishes direction for sends; receives are
framed by the <<<... Content of pcktBuf ...>>> banner.

Output: NNNN-{send|recv}.hex, one file per frame. Whitespace-separated bytes.
"""
import re
import sys
from pathlib import Path
from typing import List, Tuple

# Column guide line (always precedes hex rows)
COL_GUIDE = re.compile(r"^-+:\s+00\s+01\s+02")
# Hex data row: "00000010: 7E 1F 00 ..."
HEX_ROW = re.compile(r"^[0-9A-Fa-f]{8}:\s+((?:[0-9A-Fa-f]{2}\s*)+)$")
# Sent-frame trailer
SENT_TRAIL = re.compile(r"^\s*(\d+)\s+Bytes\s+sent", re.IGNORECASE)
# Received-frame banner
RECV_BANNER = re.compile(r"Content\s+of\s+pcktBuf", re.IGNORECASE)


def parse(lines: List[str]) -> List[Tuple[str, bytes]]:
    frames: List[Tuple[str, bytes]] = []
    current: List[int] = []
    collecting = False
    pending_direction = ""  # "recv" if we saw the banner; else assume "send"

    for line in lines:
        if RECV_BANNER.search(line):
            pending_direction = "recv"
            continue

        if COL_GUIDE.match(line):
            # New frame begins
            if current:
                direction = pending_direction or "send"
                frames.append((direction, bytes(current)))
                current = []
            collecting = True
            continue

        if collecting:
            m = HEX_ROW.match(line)
            if m:
                current.extend(int(t, 16) for t in m.group(1).split())
                continue
            s = SENT_TRAIL.match(line)
            if s and current:
                count = int(s.group(1))
                if len(current) > count:
                    current = current[:count]
                frames.append(("send", bytes(current)))
                current = []
                collecting = False
                pending_direction = ""
                continue
            # Any other line ends a receive frame
            if current:
                frames.append((pending_direction or "recv", bytes(current)))
                current = []
                collecting = False
                pending_direction = ""

    if current:
        frames.append((pending_direction or "send", bytes(current)))
    return frames


def write_fixtures(frames: List[Tuple[str, bytes]], out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    for i, (direction, data) in enumerate(frames):
        line = " ".join(f"{b:02x}" for b in data)
        (out_dir / f"{i:04d}-{direction}.hex").write_text(line + "\n")


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    input_path = Path(sys.argv[1])
    out_dir = Path(sys.argv[2])
    frames = parse(input_path.read_text(errors="replace").splitlines())
    sends = sum(1 for d, _ in frames if d == "send")
    recvs = sum(1 for d, _ in frames if d == "recv")
    print(f"parsed {len(frames)} frames ({sends} send, {recvs} recv) from {input_path}")
    write_fixtures(frames, out_dir)
    print(f"wrote {len(frames)} fixture files to {out_dir}/")
    return 0


if __name__ == "__main__":
    sys.exit(main())
