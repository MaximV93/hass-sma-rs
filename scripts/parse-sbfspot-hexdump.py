#!/usr/bin/env python3
"""Parse SBFspot's `-debug=5` stdout into clean hex-per-line frame fixtures.

Usage:
    python3 parse-sbfspot-hexdump.py INPUT.log OUTPUT_DIR/

SBFspot's HexDump() emits blocks like:

    [send] 14 Bytes:
        7e 1a 00 64 04 42 1a 5a 37 74 ff ff ff ff ff ff
        01 00 09 a0 0c 04 fd ff

Each such block becomes one frame in OUTPUT_DIR, named:

    NNNN-{send|recv}.hex

with one line = one byte pair (so they remain human-diffable).
"""
import re
import sys
from pathlib import Path
from typing import List, Tuple

# Header lines from HexDump(). SBFspot prints slightly different wording for
# send vs recv; we match both.
HEADER = re.compile(
    r"^\s*(?P<dir>(?:\[send\]|\[recv\]|sending|received))\s*"
    r"(?P<count>\d+)\s+Bytes",
    re.IGNORECASE,
)
HEX_LINE = re.compile(r"^\s*((?:[0-9a-fA-F]{2}\s*)+)$")


def parse(input_path: Path) -> List[Tuple[str, bytes]]:
    """Walk through input, collect (direction, bytes) tuples."""
    text = input_path.read_text(errors="replace")
    frames: List[Tuple[str, bytes]] = []
    current_bytes: List[int] = []
    current_dir: str = ""
    in_frame = False
    expected: int = 0

    for line in text.splitlines():
        header = HEADER.search(line)
        if header:
            if in_frame and current_bytes:
                frames.append(
                    (current_dir, bytes(current_bytes)),
                )
            in_frame = True
            raw_dir = header.group("dir").lower()
            current_dir = (
                "send" if "send" in raw_dir else "recv"
            )
            expected = int(header.group("count"))
            current_bytes = []
            continue

        if in_frame:
            m = HEX_LINE.match(line)
            if m:
                for token in m.group(1).split():
                    current_bytes.append(int(token, 16))
                if len(current_bytes) >= expected:
                    frames.append(
                        (current_dir, bytes(current_bytes[:expected])),
                    )
                    in_frame = False
                    current_bytes = []
            elif line.strip() == "":
                continue
            else:
                # Non-hex line ends frame
                if current_bytes:
                    frames.append(
                        (current_dir, bytes(current_bytes[:expected]) if expected else bytes(current_bytes)),
                    )
                in_frame = False
                current_bytes = []
    if in_frame and current_bytes:
        frames.append((current_dir, bytes(current_bytes)))
    return frames


def write_fixtures(frames: List[Tuple[str, bytes]], out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    for i, (direction, data) in enumerate(frames):
        # one byte per line, hex
        lines = " ".join(f"{b:02x}" for b in data)
        name = f"{i:04d}-{direction}.hex"
        (out_dir / name).write_text(lines + "\n")


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    input_path = Path(sys.argv[1])
    out_dir = Path(sys.argv[2])
    frames = parse(input_path)
    print(f"parsed {len(frames)} frames from {input_path}")
    sends = sum(1 for d, _ in frames if d == "send")
    recvs = sum(1 for d, _ in frames if d == "recv")
    print(f"  → {sends} sent, {recvs} received")
    write_fixtures(frames, out_dir)
    print(f"wrote fixtures to {out_dir}/")
    return 0


if __name__ == "__main__":
    sys.exit(main())
