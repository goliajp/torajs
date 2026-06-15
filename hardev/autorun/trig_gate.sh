#!/usr/bin/env bash
#
# hardev autorun pillar — TRIG-1..4 pre-trigger gate.
#
# Run by `trigger.sh self` before it generates a rotation_id and writes
# any state. Governs whether a self-initiated rotation is *legitimate*
# to trigger in the first place. Mirror of `check.sh` (INV-1..5) which
# governs whether an already-triggered rotation may *execute*.
#
# Spec: see `hardev/autorun/README.md` → "TRIG-1..4 spec" section.
# Baseline measurement: `hardev/metrics.md` §6 "Baseline @2026-06-15".
#
# Usage:
#   hardev/autorun/trig_gate.sh
#
# No arguments — gate inputs are HEAD, rotations.jsonl tail, handoff.md
# content (all resolved via lib.sh).
#
# Exit codes:
#   0  — all TRIGs PASS
#   1  — at least one TRIG FAIL; stderr summarises with `TRIG-FAILED: TRIG-x ...`
#   2  — internal error (helper missing, project dir unreadable, etc.)
#
# stdout: one line per TRIG, `TRIG-N STATE one-line-detail`. STATE is
# one of PASS / FAIL / SKIP. Lines are stable for greppability by
# trigger.sh and self-test assertions.

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
. "$SCRIPT_DIR/lib.sh"

# ── Tunables (candidate values; finalize after 1-week measure) ────────
TRIG1_MIN_COMMITS="${HARDEV_TRIG1_MIN_COMMITS:-5}"
TRIG2_MIN_WALL_SEC="${HARDEV_TRIG2_MIN_WALL_SEC:-5400}"

# Blacklist phrases (case-insensitive). Each entry = one regex; if any
# matches anywhere inside the handoff "rotate 触发" section, TRIG-4 FAILs.
# Curated from cases#rotate-as-procrastination + 2026-06-15 surface.
TRIG4_BLACKLIST=(
  'file-size .* (clear|hard limit)'
  'prep work done'
  'audit (complete|already)'
  'audit-only'
  'fresh session 更稳'
  'subagent prep 完整'
  'substantial work'
  '工程量大'
  'complexity'
  'ROI'
  'fresh ctx 更稳'
  'sub-milestone'
  'sub-phase 收口'
  'next session 起手'
)

failed=()

emit() {
  printf '%s %s %s\n' "$1" "$2" "$3"
}

# Read last self-trigger row from rotations.jsonl. Outputs tab-separated
# `ts<TAB>prevHead`. Empty string if no self row exists (cold start).
last_self_row() {
  if [ ! -f "$ROTATIONS_LOG" ]; then
    echo ""
    return
  fi
  python3 - "$ROTATIONS_LOG" "$(autorun_project_name)" <<'PY'
import json, sys
log, project = sys.argv[1], sys.argv[2]
last = None
try:
    with open(log) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                continue
            if r.get("project") != project:
                continue
            if r.get("trigger") != "self":
                continue
            last = r
except FileNotFoundError:
    pass
if last:
    print(f"{last['ts']}\t{last['prevHead']}")
PY
}

# Extract the body of the handoff's "rotate 触发" markdown section.
# Outputs lines until the next h2 (^## ) or EOF. Empty if section absent.
handoff_trigger_section() {
  if [ ! -f "$HANDOFF_FILE" ]; then
    return
  fi
  awk '
    /^## .*rotate 触发/ { in_section = 1; next }
    in_section && /^## / { in_section = 0 }
    in_section { print }
  ' "$HANDOFF_FILE"
}

# ── TRIG-1 — session commit count ≥ N ─────────────────────────────────
# `git rev-list --count <last-self-prevHead>..HEAD`. Cold start (no prior
# self row) → SKIP (treat as PASS; first rotation has no baseline).
check_trig1() {
  local row last_head head count
  row=$(last_self_row)
  if [ -z "$row" ]; then
    emit TRIG-1 SKIP "no prior self rotation (cold start)"
    return
  fi
  last_head=$(printf '%s\n' "$row" | cut -f2)
  head=$(autorun_git_head)
  if [ -z "$last_head" ] || [ -z "$head" ] || [ "$head" = "unknown" ]; then
    emit TRIG-1 FAIL "git head unresolved (last=$last_head head=$head)"
    failed+=(TRIG-1)
    return
  fi
  count=$(git -C "$PROJECT_DIR" rev-list --count "$last_head..HEAD" 2>/dev/null)
  if [ -z "$count" ]; then
    emit TRIG-1 FAIL "git rev-list failed (last=$last_head head=$head)"
    failed+=(TRIG-1)
    return
  fi
  if [ "$count" -ge "$TRIG1_MIN_COMMITS" ]; then
    emit TRIG-1 PASS "commits=$count ≥ N=$TRIG1_MIN_COMMITS"
  else
    emit TRIG-1 FAIL "commits=$count < N=$TRIG1_MIN_COMMITS (need ≥$TRIG1_MIN_COMMITS this session)"
    failed+=(TRIG-1)
  fi
}

# ── TRIG-2 — wall time ≥ M seconds ────────────────────────────────────
check_trig2() {
  local row last_ts now wall
  row=$(last_self_row)
  if [ -z "$row" ]; then
    emit TRIG-2 SKIP "no prior self rotation (cold start)"
    return
  fi
  last_ts=$(printf '%s\n' "$row" | cut -f1)
  now=$(date +%s)
  wall=$(( now - last_ts ))
  if [ "$wall" -ge "$TRIG2_MIN_WALL_SEC" ]; then
    emit TRIG-2 PASS "wall=${wall}s ≥ M=${TRIG2_MIN_WALL_SEC}s ($(( wall / 60 ))min)"
  else
    emit TRIG-2 FAIL "wall=${wall}s < M=${TRIG2_MIN_WALL_SEC}s ($(( wall / 60 ))min vs $(( TRIG2_MIN_WALL_SEC / 60 ))min)"
    failed+=(TRIG-2)
  fi
}

# ── TRIG-3 — handoff "rotate 触发" reason ∈ {a,b,c,d} ─────────────────
check_trig3() {
  if [ ! -f "$HANDOFF_FILE" ]; then
    emit TRIG-3 FAIL "handoff.md missing at $HANDOFF_FILE"
    failed+=(TRIG-3)
    return
  fi
  local body reason
  body=$(handoff_trigger_section)
  if [ -z "$body" ]; then
    emit TRIG-3 FAIL "handoff missing '## rotate 触发' section"
    failed+=(TRIG-3)
    return
  fi
  # Match `reason: a` / `reason: b` / `reason: c` / `reason: d` (case-insensitive,
  # one of the closed enum letters; allow surrounding whitespace).
  reason=$(printf '%s\n' "$body" | grep -iE '^[[:space:]]*reason:[[:space:]]*[abcd][[:space:]]*$' | head -1)
  if [ -z "$reason" ]; then
    emit TRIG-3 FAIL "no 'reason: <a|b|c|d>' line in rotate-trigger section"
    failed+=(TRIG-3)
    return
  fi
  emit TRIG-3 PASS "reason matched: $(printf '%s' "$reason" | tr -d '[:space:]')"
}

# ── TRIG-4 — blacklist phrase guard ────────────────────────────────────
check_trig4() {
  if [ ! -f "$HANDOFF_FILE" ]; then
    emit TRIG-4 FAIL "handoff.md missing at $HANDOFF_FILE"
    failed+=(TRIG-4)
    return
  fi
  local body hits=()
  body=$(handoff_trigger_section)
  if [ -z "$body" ]; then
    emit TRIG-4 SKIP "no rotate-trigger section (TRIG-3 already FAIL)"
    return
  fi
  local p
  for p in "${TRIG4_BLACKLIST[@]}"; do
    if printf '%s\n' "$body" | grep -iqE "$p"; then
      hits+=("$p")
    fi
  done
  if [ "${#hits[@]}" -eq 0 ]; then
    emit TRIG-4 PASS "0 blacklist phrase hits"
  else
    emit TRIG-4 FAIL "blacklist hit: ${hits[*]}"
    failed+=(TRIG-4)
  fi
}

check_trig1
check_trig2
check_trig3
check_trig4

if [ "${#failed[@]}" -gt 0 ]; then
  echo "TRIG-FAILED: ${failed[*]}" >&2
  exit 1
fi
exit 0
