#!/usr/bin/env bash
#
# hardev autorun pillar — INV-1..5 pre-act gate.
#
# Run by the Stop hook (P1.2) before writing `.claude/autorun-marker`,
# and again by the watcher (P1.3) before sending `tmux send-keys`. Two
# call sites = defense-in-depth: the Stop hook gates marker creation;
# the watcher gates the act of clearing the user's session. If either
# fails, the rotation does NOT proceed.
#
# Usage:
#   hardev/autorun/check.sh [rotation_id]
#
# rotation_id is optional. When present, INV-5 (uniqueness) runs against
# `rotations.jsonl`. When absent, INV-5 is skipped (still PASS, marked
# "skipped").
#
# Exit codes:
#   0  — all applicable INVs PASS
#   1  — at least one INV FAIL; stderr summarises with `FAILED: INV-x ...`
#   2  — internal error (helper missing, project dir unreadable, etc.)
#
# stdout: one line per INV, `INV-N STATE one-line-detail`. STATE is one
# of PASS / FAIL / SKIP. Lines are stable for greppability by P1.2/P1.3
# and for self-test assertions.
#
# stderr (FAIL only): a `FAILED: INV-N [INV-M ...]` summary line, plus
# any extra context the per-INV check emitted.

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
. "$SCRIPT_DIR/lib.sh"

ROTATION_ID="${1:-}"

failed=()

emit() {
  # INV-N STATE detail
  printf '%s %s %s\n' "$1" "$2" "$3"
}

# ── INV-1 — handoff.md mtime age < 90 s ─────────────────────────────────
# Why: row #6 of the P0 baseline (`r-1779265047-549c`) recorded a
# handoffAgeSec of 7489 — handoff.md was 2 h stale relative to
# trigger.sh. The new session would resume on a handoff that describes
# state preceding the actual prevHead. 90 s matches README Layer 1
# planned threshold (allows the agent to save then trigger inside the
# same turn).
check_inv1() {
  if [ ! -f "$HANDOFF_FILE" ]; then
    emit INV-1 FAIL "handoff.md missing at $HANDOFF_FILE"
    failed+=(INV-1)
    return
  fi
  local age
  age=$(autorun_file_age_sec "$HANDOFF_FILE")
  if [ -z "$age" ]; then
    emit INV-1 FAIL "could not stat handoff.md"
    failed+=(INV-1)
    return
  fi
  if [ "$age" -lt 90 ]; then
    emit INV-1 PASS "handoff.md age ${age}s (<90)"
  else
    emit INV-1 FAIL "handoff.md age ${age}s >= 90 (stale)"
    failed+=(INV-1)
  fi
}

# ── INV-2 — working tree clean ──────────────────────────────────────────
# Why: rotation is about to /clear the session. Any uncommitted change
# (unstaged or staged-but-uncommitted) becomes invisible to the new
# session — the handoff.md narrates committed state. Forcing clean tree
# converts the silent loss into a loud gate failure.
check_inv2() {
  local porcelain
  porcelain=$(git -C "$PROJECT_DIR" status --porcelain 2>/dev/null) || {
    emit INV-2 FAIL "git status failed (not a repo?)"
    failed+=(INV-2)
    return
  }
  if [ -z "$porcelain" ]; then
    emit INV-2 PASS "tree clean"
  else
    local n
    n=$(printf '%s\n' "$porcelain" | wc -l | tr -d ' ')
    emit INV-2 FAIL "tree dirty: ${n} entr$([ "$n" -eq 1 ] && echo y || echo ies)"
    failed+=(INV-2)
  fi
}

# ── INV-3 — conformance non-decreasing vs last jsonl row ────────────────
# Why: P0 baseline already exhibits monotonic-non-decreasing across 10
# rows; P1 turns the observation into a machine gate. Reading the
# *current* conformance from status memory header is best-effort (same
# source `autorun_record_rotation` uses for `conformanceBefore`), so
# the INV-3 check matches what the next row would record.
check_inv3() {
  local current_conf last_conf last_pass cur_pass
  current_conf=$(autorun_conformance_now)
  if [ -z "$current_conf" ]; then
    emit INV-3 SKIP "no current conformance reading"
    return
  fi
  if [ ! -s "$ROTATIONS_LOG" ]; then
    emit INV-3 PASS "current $current_conf (no prior row)"
    return
  fi
  last_conf=$(tail -1 "$ROTATIONS_LOG" | python3 -c '
import json, sys
try:
    row = json.loads(sys.stdin.read().strip())
    v = row.get("conformanceBefore")
    print(v if v else "")
except Exception:
    print("")
' 2>/dev/null)
  if [ -z "$last_conf" ]; then
    emit INV-3 PASS "current $current_conf (prior row had null conf)"
    return
  fi
  cur_pass=$(printf '%s' "$current_conf" | cut -d/ -f1)
  last_pass=$(printf '%s' "$last_conf" | cut -d/ -f1)
  if [ "$cur_pass" -ge "$last_pass" ]; then
    emit INV-3 PASS "current $current_conf >= prior $last_conf"
  else
    emit INV-3 FAIL "current $current_conf < prior $last_conf (regression)"
    failed+=(INV-3)
  fi
}

# ── INV-4 — handoff.md non-empty + has `> saved:` metadata line ─────────
# Why: file existence + mtime are not enough. A 0-byte handoff, or one
# missing the metadata blockquote, indicates either a half-written save
# or stray `touch`. Both are "phantom" handoffs that would mislead a
# new session.
check_inv4() {
  if [ ! -f "$HANDOFF_FILE" ]; then
    emit INV-4 FAIL "handoff.md missing"
    failed+=(INV-4)
    return
  fi
  if [ ! -s "$HANDOFF_FILE" ]; then
    emit INV-4 FAIL "handoff.md empty"
    failed+=(INV-4)
    return
  fi
  if ! grep -q '^> saved:' "$HANDOFF_FILE"; then
    emit INV-4 FAIL "handoff.md missing '> saved:' metadata blockquote"
    failed+=(INV-4)
    return
  fi
  local bytes
  bytes=$(wc -c < "$HANDOFF_FILE" | tr -d ' ')
  emit INV-4 PASS "handoff.md ${bytes}B with metadata blockquote"
}

# ── INV-5 — rotation_id not yet present in rotations.jsonl ──────────────
# Why: rotation_id is `r-<unix-ts>-<4 hex>`, so same-second collisions
# collapse to 1 / 65536. Tiny absolute risk, near-zero cost to guard.
# Without the guard, a duplicate id would silently corrupt downstream
# audit / dashboard joins.
check_inv5() {
  if [ -z "$ROTATION_ID" ]; then
    emit INV-5 SKIP "no rotation_id provided"
    return
  fi
  if [ ! -s "$ROTATIONS_LOG" ]; then
    emit INV-5 PASS "rotation_id $ROTATION_ID unique (empty log)"
    return
  fi
  if grep -q "\"rotationId\":\"$ROTATION_ID\"" "$ROTATIONS_LOG"; then
    emit INV-5 FAIL "rotation_id $ROTATION_ID already present in jsonl"
    failed+=(INV-5)
    return
  fi
  emit INV-5 PASS "rotation_id $ROTATION_ID unique"
}

# ── run ─────────────────────────────────────────────────────────────────
check_inv1
check_inv2
check_inv3
check_inv4
check_inv5

if [ ${#failed[@]} -eq 0 ]; then
  exit 0
fi

printf 'FAILED: %s\n' "${failed[*]}" >&2
exit 1
