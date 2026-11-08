#!/usr/bin/env bash
# Runtime socket test (docs/04-privacy-and-security.md, guarantee 1).
#
# Runs the core test suite under strace and fails if any test process creates
# a network socket. The core tests build recordings on disk, save and reload
# them through SQLite, detect and redact sensitive text, and export Markdown
# and HTML, so this covers the storage and export path of a session.
#
# Linux only (strace). The Windows capture code is not covered here: it needs
# a real desktop. See the "Not covered" notes in docs/04-privacy-and-security.md.
#
# Before trusting a pass, the script proves the detector works by tracing a
# tiny program that does open a socket and checking it is caught.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Any socket that is not a local Unix socket counts. glibc can use Unix
# sockets for user and host lookups on the machine itself; those never leave it.
calls='socket,socketpair,connect,bind,sendto,sendmsg'
network_lines() { grep -E '(socket|socketpair)\(' "$1" | grep -v 'AF_UNIX' || true; }

trace() { # trace <log> <command...>
  local log="$1"; shift
  strace -f -qq -e trace="$calls" -o "$log" "$@"
}

# 1. Prove the detector can fail.
trace "$work/canary.log" python3 -c 'import socket; socket.socket(socket.AF_INET, socket.SOCK_DGRAM).close()'
if [ -z "$(network_lines "$work/canary.log")" ]; then
  echo "no-socket: FAIL - the detector missed a planted socket, so a pass would mean nothing"
  exit 1
fi

# 2. Build the test binaries, then trace each one directly (not cargo itself).
packages=(-p babelsteps-core -p babelsteps-privacy-checks)
cargo test "${packages[@]}" --no-run --message-format=json 2>/dev/null \
  | python3 -c '
import json, sys
for line in sys.stdin:
    try: m = json.loads(line)
    except ValueError: continue
    if m.get("reason") == "compiler-artifact" and m.get("profile", {}).get("test") and m.get("executable"):
        print(m["executable"])
' > "$work/bins"

count=$(wc -l < "$work/bins")
if [ "$count" -lt 3 ]; then
  echo "no-socket: FAIL - found only $count test binaries; expected the core unit, core integration and privacy tests"
  exit 1
fi

fail=0
i=0
while read -r bin; do
  i=$((i + 1))
  log="$work/run-$i.log"
  if ! trace "$log" "$bin" --quiet > "$work/out-$i.txt" 2>&1; then
    echo "no-socket: tests failed in $(basename "$bin")"; tail -20 "$work/out-$i.txt"; fail=1; continue
  fi
  passed=$(grep -Eo '[0-9]+ passed' "$work/out-$i.txt" | awk '{s+=$1} END {print s+0}')
  hits="$(network_lines "$log")"
  if [ -n "$hits" ]; then
    echo "no-socket: FAIL - $(basename "$bin") opened a network socket:"
    echo "$hits" | head -10
    fail=1
  else
    echo "no-socket: ok - $(basename "$bin") ($passed tests, no network socket)"
  fi
done < "$work/bins"

if [ "$fail" -ne 0 ]; then exit 1; fi
echo "no-socket: PASS ($count test binaries traced)"
