#!/usr/bin/env bash
#
# hardev autorun pillar — check.sh self-test.
#
# Verifies the 4 canonical paths through `check.sh` end-to-end:
#
#   case 1: GREEN happy path                    → exit 0, INV-1..5 all PASS or SKIP
#   case 2: INV-1 (stale handoff.md mtime)      → exit 1, INV-1 FAIL line
#   case 3: INV-2 (dirty working tree)          → exit 1, INV-2 FAIL line
#   case 4: INV-5 (duplicate rotation_id)       → exit 1, INV-5 FAIL line
#
# INV-3 (conformance non-decreasing) and INV-4 (handoff metadata) FAIL
# cases require touching the real status memory or corrupting handoff.md
# in non-trivial ways; they are exercised by the GREEN happy path (which
# requires both to PASS), so an INV-3/4 regression in `check.sh` is
# caught by case 1 flipping from PASS to FAIL.
#
# Side effects (all trap-restored):
#   - handoff.md mtime is temporarily rolled back, then `touch -m`ed
#     forward; final state has mtime ≈ now (still valid, content
#     unchanged).
#   - One fake file briefly appears under hardev/autorun/.selftest-marker
#     to simulate a dirty tree, then is rm-f'd.
#
# Exit: 0 if all 4 cases behave as expected; 1 otherwise.

set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CHECK="$SCRIPT_DIR/check.sh"
# shellcheck source=lib.sh
. "$SCRIPT_DIR/lib.sh"

if [ ! -x "$CHECK" ]; then
  echo "self-test: check.sh missing or not executable at $CHECK" >&2
  exit 2
fi
if [ ! -f "$HANDOFF_FILE" ]; then
  echo "self-test: $HANDOFF_FILE missing — cannot run without a real handoff" >&2
  exit 2
fi

orig_mtime=$(stat -f %m "$HANDOFF_FILE")
fake_dirty="$AUTORUN_DIR/.selftest-marker"

cleanup() {
  if [ -f "$HANDOFF_FILE" ] && [ -n "${orig_mtime:-}" ]; then
    local ts
    ts=$(date -r "$orig_mtime" +%Y%m%d%H%M.%S 2>/dev/null) || ts=""
    [ -n "$ts" ] && touch -t "$ts" "$HANDOFF_FILE" 2>/dev/null || true
  fi
  rm -f "$fake_dirty"
}
trap cleanup EXIT INT TERM

pass=0
fail=0

run_case() {
  # run_case <name> <expected_exit> <expected_grep_pattern_or_empty> <cmd...>
  local name=$1 expected_exit=$2 pattern=$3
  shift 3
  local out rc
  out=$("$@" 2>&1) && rc=0 || rc=$?
  if [ "$rc" -ne "$expected_exit" ]; then
    printf 'FAIL %s: expected exit=%d, got exit=%d\n' "$name" "$expected_exit" "$rc"
    printf '%s\n' "  output:" "$out" | sed 's/^/    /'
    fail=$((fail + 1))
    return
  fi
  if [ -n "$pattern" ] && ! printf '%s' "$out" | grep -q -- "$pattern"; then
    printf 'FAIL %s: exit ok but output missing pattern "%s"\n' "$name" "$pattern"
    printf '%s\n' "  output:" "$out" | sed 's/^/    /'
    fail=$((fail + 1))
    return
  fi
  printf 'PASS %s\n' "$name"
  pass=$((pass + 1))
}

# ── case 1 — GREEN happy path ───────────────────────────────────────────
# Requires: tree clean (caller's job), handoff fresh (we touch -m it).
echo "[case 1] GREEN happy path → exit 0 + all INV PASS/SKIP"
touch -m "$HANDOFF_FILE"
run_case "case-1 GREEN happy" 0 "INV-1 PASS" "$CHECK"

# ── case 2 — INV-1 stale handoff ────────────────────────────────────────
echo "[case 2] INV-1 stale handoff (-200s mtime) → exit 1 + INV-1 FAIL"
touch -t "$(date -v-200S +%Y%m%d%H%M.%S)" "$HANDOFF_FILE"
run_case "case-2 INV-1 stale" 1 "INV-1 FAIL" "$CHECK"
touch -m "$HANDOFF_FILE"

# ── case 3 — INV-2 dirty tree ───────────────────────────────────────────
echo "[case 3] INV-2 dirty tree (fake marker file) → exit 1 + INV-2 FAIL"
echo "self-test marker; rm on cleanup" > "$fake_dirty"
run_case "case-3 INV-2 dirty" 1 "INV-2 FAIL" "$CHECK"
rm -f "$fake_dirty"

# ── case 4 — INV-5 duplicate rotation_id ────────────────────────────────
echo "[case 4] INV-5 duplicate rotation_id (most-recent jsonl id) → exit 1 + INV-5 FAIL"
if [ ! -s "$ROTATIONS_LOG" ]; then
  echo "  SKIP: rotations.jsonl empty, cannot test duplicate id"
else
  dup_id=$(tail -1 "$ROTATIONS_LOG" | python3 -c '
import json, sys
print(json.loads(sys.stdin.read().strip())["rotationId"])
' 2>/dev/null)
  if [ -z "$dup_id" ]; then
    echo "  SKIP: could not extract rotationId from last jsonl row"
  else
    run_case "case-4 INV-5 dup-id" 1 "INV-5 FAIL" "$CHECK" "$dup_id"
  fi
fi

echo
echo "self-test: $pass pass · $fail fail"
[ "$fail" -eq 0 ]
