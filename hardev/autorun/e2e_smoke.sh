#!/usr/bin/env bash
#
# hardev autorun pillar — end-to-end smoke test.
#
# Walks the stop_hook → marker pipeline three times, exercising the
# scenarios that the per-script unit tests (check_self_test.sh) cannot
# cover end-to-end:
#
#   case 1: GREEN happy path           → marker written + intent consumed
#   case 2: INV-1 stale handoff        → intent kept + marker NOT written
#   case 3: stale-intent regression    → rid already present in jsonl,
#                                        stop_hook still GREEN-lits
#                                        (regression guard for 9346fa5)
#
# Why this exists: P1.5 dogfood was takagi running 5 manual rotations
# in a tmux sink. Re-running that by hand for every autorun pipeline
# commit is friction. This script automates the smoke layer — same
# stop_hook + check.sh path, but the marker is consumed locally and
# the watcherd dry-run pane is skipped (launchd will still wake the
# watcher when the marker file is touched; the smoke cleanup waits
# for that one-shot to finish before tearing down).
#
# All sentinels are scoped: the test backs up any pre-existing
# .claude/autorun-intent / .claude/autorun-marker before running and
# restores them on exit. handoff.md mtime is restored too. rotations.jsonl
# is NEVER written — the smoke test bypasses trigger.sh and synthesises
# the intent file directly with a `smoketest-` rid prefix.
#
# Exit: 0 if all cases behave as expected; 1 if any case fails; 2 on
# precondition violation (e.g. dirty tree).

set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
. "$SCRIPT_DIR/lib.sh"

if [ ! -x "$SCRIPT_DIR/stop_hook.sh" ]; then
  echo "smoke: stop_hook.sh missing or not executable at $SCRIPT_DIR" >&2
  exit 2
fi
if [ ! -f "$HANDOFF_FILE" ]; then
  echo "smoke: $HANDOFF_FILE missing — cannot run without a real handoff" >&2
  exit 2
fi

# Precondition: tree must be clean. INV-2 is one of the checks the
# stop_hook walks; running smoke on a dirty tree would dilute the
# pass/fail signal (case 1 would fail for the wrong reason).
if [ -n "$(git -C "$PROJECT_DIR" status --porcelain 2>/dev/null)" ]; then
  echo "smoke: working tree is dirty — commit or stash first; INV-2 conflates real changes with the test scenario" >&2
  exit 2
fi

# ── snapshot + cleanup machinery ────────────────────────────────────────
orig_intent_present=false
orig_marker_present=false
if [ -f "$INTENT_FILE" ]; then
  orig_intent_present=true
  cp "$INTENT_FILE" "$INTENT_FILE.smoke-bak"
fi
if [ -f "$MARKER_FILE" ]; then
  orig_marker_present=true
  cp "$MARKER_FILE" "$MARKER_FILE.smoke-bak"
fi
orig_handoff_mtime=$(stat -f %m "$HANDOFF_FILE" 2>/dev/null || echo "")

cleanup() {
  # Wait briefly so any launchd-spawned watcherd cycle finishes
  # touching err.log before we move on. 3 s is generous on macOS
  # (WatchPaths typically debounces under 1 s).
  sleep 3
  rm -f "$INTENT_FILE" "$MARKER_FILE"
  if [ "$orig_intent_present" = true ] && [ -f "$INTENT_FILE.smoke-bak" ]; then
    mv "$INTENT_FILE.smoke-bak" "$INTENT_FILE"
  fi
  if [ "$orig_marker_present" = true ] && [ -f "$MARKER_FILE.smoke-bak" ]; then
    mv "$MARKER_FILE.smoke-bak" "$MARKER_FILE"
  fi
  if [ -n "$orig_handoff_mtime" ]; then
    local ts
    ts=$(date -r "$orig_handoff_mtime" +%Y%m%d%H%M.%S 2>/dev/null) || ts=""
    [ -n "$ts" ] && touch -t "$ts" "$HANDOFF_FILE" 2>/dev/null
  fi
}
trap cleanup EXIT INT TERM

pass=0
fail=0

run_case() {
  # run_case <name> <fn>
  local name=$1 fn=$2
  if "$fn"; then
    printf 'PASS %s\n' "$name"
    pass=$((pass + 1))
  else
    printf 'FAIL %s\n' "$name"
    fail=$((fail + 1))
  fi
}

mk_fake_rid() {
  printf 'r-smoketest-%s-%s' "$1" "$(date +%s)"
}

# ── case 1 — GREEN happy path ───────────────────────────────────────────
case_green_happy() {
  rm -f "$INTENT_FILE" "$MARKER_FILE"
  touch -m "$HANDOFF_FILE"
  printf '%s\n' "$(mk_fake_rid green)" > "$INTENT_FILE"
  local log
  log=$(bash "$SCRIPT_DIR/stop_hook.sh" 2>&1)
  if [ -f "$INTENT_FILE" ]; then
    echo "  intent kept (expected consumed). stop_hook output:" >&2
    printf '%s\n' "$log" | sed 's/^/    /' >&2
    return 1
  fi
  if [ ! -f "$MARKER_FILE" ]; then
    echo "  marker missing (expected written). stop_hook output:" >&2
    printf '%s\n' "$log" | sed 's/^/    /' >&2
    return 1
  fi
  if ! printf '%s' "$log" | grep -q 'INV-5 SKIP'; then
    echo "  expected INV-5 SKIP line (stop_hook must call check.sh without rid). output:" >&2
    printf '%s\n' "$log" | sed 's/^/    /' >&2
    return 1
  fi
  rm -f "$MARKER_FILE"
}

# ── case 2 — INV-1 stale handoff ────────────────────────────────────────
case_inv1_stale() {
  rm -f "$INTENT_FILE" "$MARKER_FILE"
  touch -t "$(date -v-200S +%Y%m%d%H%M.%S)" "$HANDOFF_FILE"
  printf '%s\n' "$(mk_fake_rid stale)" > "$INTENT_FILE"
  local log
  log=$(bash "$SCRIPT_DIR/stop_hook.sh" 2>&1)
  if [ ! -f "$INTENT_FILE" ]; then
    echo "  intent consumed on stale handoff (expected kept). output:" >&2
    printf '%s\n' "$log" | sed 's/^/    /' >&2
    return 1
  fi
  if [ -f "$MARKER_FILE" ]; then
    echo "  marker written on stale handoff (expected absent). output:" >&2
    printf '%s\n' "$log" | sed 's/^/    /' >&2
    return 1
  fi
  if ! printf '%s' "$log" | grep -q 'INV-1 FAIL'; then
    echo "  expected INV-1 FAIL line. output:" >&2
    printf '%s\n' "$log" | sed 's/^/    /' >&2
    return 1
  fi
  rm -f "$INTENT_FILE"
  touch -m "$HANDOFF_FILE"
}

# ── case 3 — stale-intent loop regression guard (rid already in jsonl) ─
# Pre-9346fa5: stop_hook passed the rid to check.sh, so any rid already
# in jsonl tripped INV-5 → RED loop forever. This case writes a rid
# that's guaranteed to be in jsonl (tail row) and asserts stop_hook
# still GREEN-lits (intent consumed + marker written).
case_stale_intent_regression() {
  if [ ! -s "$ROTATIONS_LOG" ]; then
    echo "  SKIP: rotations.jsonl empty, cannot replay duplicate rid"
    return 0
  fi
  local dup_rid
  dup_rid=$(tail -1 "$ROTATIONS_LOG" | python3 -c '
import json, sys
print(json.loads(sys.stdin.read().strip())["rotationId"])' 2>/dev/null || echo "")
  if [ -z "$dup_rid" ]; then
    echo "  SKIP: could not extract last rotationId from jsonl"
    return 0
  fi
  rm -f "$INTENT_FILE" "$MARKER_FILE"
  touch -m "$HANDOFF_FILE"
  printf '%s\n' "$dup_rid" > "$INTENT_FILE"
  local log
  log=$(bash "$SCRIPT_DIR/stop_hook.sh" 2>&1)
  if [ -f "$INTENT_FILE" ]; then
    echo "  intent kept on duplicate rid → stale-intent loop regressed. output:" >&2
    printf '%s\n' "$log" | sed 's/^/    /' >&2
    return 1
  fi
  if [ ! -f "$MARKER_FILE" ]; then
    echo "  marker missing on duplicate rid. output:" >&2
    printf '%s\n' "$log" | sed 's/^/    /' >&2
    return 1
  fi
  rm -f "$MARKER_FILE"
}

# ── run ────────────────────────────────────────────────────────────────
echo "[case 1] GREEN happy path → marker written + intent consumed"
run_case "case-1 GREEN happy → marker + intent consumed" case_green_happy

echo "[case 2] INV-1 stale handoff (-200s mtime) → intent kept + no marker"
run_case "case-2 INV-1 stale → intent kept + no marker" case_inv1_stale

echo "[case 3] stale-intent regression — rid already in jsonl → still GREEN-lits"
run_case "case-3 stale-intent regression → marker still written" case_stale_intent_regression

echo
echo "smoke: $pass pass · $fail fail"
[ $fail -eq 0 ]
