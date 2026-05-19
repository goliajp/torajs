#!/usr/bin/env bash
#
# hardev autorun pillar — render rotations.jsonl as a readable table.
#
# Usage:
#   hardev/autorun/log.sh                  # all rotations, newest last
#   hardev/autorun/log.sh --tail 10        # only the last 10
#   hardev/autorun/log.sh --json           # raw JSONL passthrough (audit)
#
# Output (default human-readable):
#   ROTATION_ID            WHEN                  TRIGGER  HEAD     HANDOFF  CONF
#   r-1747836296-a1b2      2026-05-20T12:34:56Z  manual   aaaef71  12s      631/0/1
#   ...

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/lib.sh"

TAIL=""
RAW=0
while [ $# -gt 0 ]; do
  case "$1" in
    --tail)
      shift
      TAIL="${1:-}"
      if ! [[ "$TAIL" =~ ^[0-9]+$ ]]; then
        echo "log.sh: --tail requires a positive integer" >&2
        exit 2
      fi
      ;;
    --json) RAW=1 ;;
    -h|--help)
      sed -n '3,12p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "log.sh: unknown arg '$1'" >&2
      exit 2
      ;;
  esac
  shift
done

if [ ! -f "$ROTATIONS_LOG" ]; then
  echo "hardev autorun: no rotations recorded yet ($ROTATIONS_LOG missing)"
  echo "  run hardev/autorun/trigger.sh to record the first one."
  exit 0
fi

if [ "$RAW" = "1" ]; then
  if [ -n "$TAIL" ]; then
    tail -n "$TAIL" "$ROTATIONS_LOG"
  else
    cat "$ROTATIONS_LOG"
  fi
  exit 0
fi

# Human render via python3 (macOS built-in). Tolerant of schema additions
# (unknown fields silently dropped); missing fields render as `—`.
TAIL="$TAIL" python3 - "$ROTATIONS_LOG" <<'PY'
import json, os, sys

path = sys.argv[1]
tail_env = os.environ.get("TAIL", "")
tail = int(tail_env) if tail_env else None

rows = []
with open(path) as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            # Skip malformed lines; do not silently lose count.
            sys.stderr.write(f"hardev autorun log: skipping malformed line: {line[:80]}...\n")

if tail is not None:
    rows = rows[-tail:]

if not rows:
    print("hardev autorun: rotations.jsonl present but empty")
    sys.exit(0)

def cell(v):
    if v is None or v == "":
        return "—"
    return str(v)

def short_sha(s):
    if not s or not isinstance(s, str):
        return "—"
    if s.startswith("sha256:"):
        return s[7:15]
    return s[:8]

fmt = "  {rid:<22}  {at:<22}  {trg:<8}  {head:<8}  {hage:<7}  {conf}"
print(fmt.format(rid="ROTATION_ID", at="WHEN", trg="TRIGGER", head="HEAD", hage="HANDOFF", conf="CONF"))
print(fmt.format(rid="-" * 22, at="-" * 22, trg="-" * 8, head="-" * 8, hage="-" * 7, conf="-" * 8))
for r in rows:
    age = r.get("handoffAgeSec")
    age_s = "—" if age is None else f"{age}s"
    print(fmt.format(
        rid=cell(r.get("rotationId")),
        at=cell(r.get("at")),
        trg=cell(r.get("trigger")),
        head=cell(r.get("prevHead")),
        hage=age_s,
        conf=cell(r.get("conformanceBefore")),
    ))

print()
print(f"  {len(rows)} rotation(s)" + (f" (tail {tail})" if tail else ""))
PY
