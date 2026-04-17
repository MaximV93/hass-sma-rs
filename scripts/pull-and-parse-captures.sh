#!/usr/bin/env bash
#
# Pull SBFspot debug logs from the live HA (via SSH), parse hex dumps into
# per-frame fixtures, run the Frame::parse validation test.
#
# Assumes:
# - `ssh hassio@192.168.101.15` works without password (key-based).
# - `/share/sbfspot-logs/sbfspot-YYYY-MM-DD.log` exists on HA with `-d5`
#   SBFspot runs.
# - `tests/fixtures/captured/` is where fixture files live.
#
# Usage:  scripts/pull-and-parse-captures.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STAGING="$(mktemp -d -t sma-caps.XXXXXX)"
FIXTURES_DIR="${REPO_ROOT}/tests/fixtures/captured"

trap 'rm -rf "${STAGING}"' EXIT

echo "[1/4] Pulling /share/sbfspot-logs/ from HA..."
scp -r hassio@192.168.101.15:/share/sbfspot-logs/ "${STAGING}/"
ls -l "${STAGING}/sbfspot-logs/"

echo "[2/4] Parsing hex dumps into ${FIXTURES_DIR}/"
mkdir -p "${FIXTURES_DIR}"
rm -f "${FIXTURES_DIR}"/*.hex  # fresh every run
for log in "${STAGING}"/sbfspot-logs/*.log; do
    echo "  → ${log}"
    python3 "${REPO_ROOT}/scripts/parse-sbfspot-hexdump.py" "${log}" "${FIXTURES_DIR}"
done
count=$(ls -1 "${FIXTURES_DIR}"/*.hex 2>/dev/null | wc -l)
echo "   ${count} fixture files generated"

if [ "${count}" = "0" ]; then
    echo "WARNING: no fixtures. Check the log for 'Bytes:' hex blocks."
    exit 1
fi

echo "[3/4] cargo test -p sma-bt-protocol --test captured_frames"
( cd "${REPO_ROOT}" && cargo test -p sma-bt-protocol --test captured_frames 2>&1 | tail -10 )

echo "[4/4] Done. Fixtures staged at ${FIXTURES_DIR}/."
