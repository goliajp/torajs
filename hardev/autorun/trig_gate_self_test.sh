#!/usr/bin/env bash
#
# hardev autorun pillar — trig_gate.sh self-test.
#
# The TRIG gate had no test until 2026-09-07, which is part of why two
# of its inputs could rot unnoticed (`commitsInSession` hardcoded null,
# `conformanceBefore` probing a profile layout two generations dead).
# Every branch that can block or release a rotation is exercised here
# against fixtures — `HARDEV_ROTATIONS_LOG` / `HARDEV_HANDOFF_FILE`
# redirect lib.sh's paths, so production state is never touched.
#
# Cases:
#   1  happy path                       → exit 0, TRIG-1..5 PASS
#   2  too few commits                  → exit 1, TRIG-1 FAIL
#   3  too few commits + over wall cap   → exit 0, TRIG-1 WAIVED
#   4  wall under the floor              → exit 1, TRIG-2 FAIL
#   5  missing axis:                     → exit 1, TRIG-3 FAIL
#   6  closed: sha outside session range → exit 1, TRIG-3 FAIL
#   7  gate: triple with a nonzero fail  → exit 1, TRIG-3 FAIL
#   8  blacklist phrase                  → exit 1, TRIG-4 FAIL
#   9  8th consecutive same-axis, no review → exit 1, TRIG-5 FAIL
#  10  same, with axes-review:           → exit 0, TRIG-5 PASS
#
# Exit: 0 if all cases behave as expected; 1 otherwise.

set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GATE="$SCRIPT_DIR/trig_gate.sh"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

pass=0
fail=0

# A sha that really is in this repo's recent history, and one that is not.
REAL_SHA="$(git -C "$PROJECT_DIR" rev-parse --short HEAD)"
OLD_SHA="$(git -C "$PROJECT_DIR" rev-list HEAD | tail -1 | cut -c1-9)"

# Write a rotations.jsonl of self rows sitting `age` seconds back and
# `n` commits behind HEAD. Any further arguments are the axis values,
# one row each, oldest first — the LAST one is the most recent rotation,
# which is the row lib.sh reads for wall time and commit count. With no
# axis arguments a single axis-less row is written (the pre-2026-09-07
# schema, which TRIG-5 must treat as unknown rather than as a streak).
mk_log() {
  local file=$1 age=$2 n=$3; shift 3
  local ts base
  ts=$(( $(date +%s) - age ))
  base=$(git -C "$PROJECT_DIR" rev-parse --short "HEAD~$n")
  : > "$file"
  if [ "$#" -eq 0 ]; then
    set -- null
  fi
  local a
  for a in "$@"; do
    printf '{"rotationId":"r-x","at":"x","ts":%d,"project":"torajs","trigger":"self","prevHead":"%s","axis":%s}\n' \
      "$ts" "$base" "$a" >> "$file"
  done
}

mk_handoff() {
  printf '%s\n' "## rotate 触发" "" "$@" > "$TMP/handoff.md"
}

run_case() {
  local name=$1 want_exit=$2 want_line=$3
  local out rc
  out=$(HARDEV_ROTATIONS_LOG="$TMP/rot.jsonl" HARDEV_HANDOFF_FILE="$TMP/handoff.md" \
        bash "$GATE" 2>/dev/null)
  rc=$?
  if [ "$rc" -eq "$want_exit" ] && printf '%s\n' "$out" | grep -qE "$want_line"; then
    printf 'ok   %s\n' "$name"
    pass=$(( pass + 1 ))
  else
    printf 'FAIL %s (exit=%d want=%d; wanted line /%s/)\n' "$name" "$rc" "$want_exit" "$want_line"
    printf '%s\n' "$out" | sed 's/^/       | /'
    fail=$(( fail + 1 ))
  fi
}

GOOD_AXIS="axis: A"
GOOD_CLOSED="closed: $REAL_SHA did a thing"
GOOD_GATE="gate: 3773/0/4"

# 1 — happy path: 12 commits, 2h wall, full section.
mk_log "$TMP/rot.jsonl" 7200 12
mk_handoff "$GOOD_AXIS" "$GOOD_CLOSED" "$GOOD_GATE"
run_case "1 happy path" 0 'TRIG-1 PASS'

# 2 — six commits is no longer enough.
mk_log "$TMP/rot.jsonl" 7200 6
run_case "2 too few commits" 1 'TRIG-1 FAIL'

# 3 — same six commits, but past the wall cap: the escape hatch.
mk_log "$TMP/rot.jsonl" 19000 6
run_case "3 over cap waives TRIG-1" 0 'TRIG-1 WAIVED'

# 4 — twelve commits inside twenty minutes is a batch of trivia.
mk_log "$TMP/rot.jsonl" 1200 12
run_case "4 under wall floor" 1 'TRIG-2 FAIL'

# 5 — no axis line.
mk_log "$TMP/rot.jsonl" 7200 12
mk_handoff "$GOOD_CLOSED" "$GOOD_GATE"
run_case "5 missing axis" 1 'TRIG-3 FAIL'

# 6 — a sha, but not one from this session.
mk_handoff "$GOOD_AXIS" "closed: $OLD_SHA ancient history" "$GOOD_GATE"
run_case "6 closed sha out of range" 1 'TRIG-3 FAIL'

# 7 — the gate is red.
mk_handoff "$GOOD_AXIS" "$GOOD_CLOSED" "gate: 3773/2/4"
run_case "7 red gate" 1 'TRIG-3 FAIL'

# 8 — a blacklisted phrasing.
mk_handoff "$GOOD_AXIS" "$GOOD_CLOSED" "$GOOD_GATE" "ROI 不高所以先停"
run_case "8 blacklist phrase" 1 'TRIG-4 FAIL'

# 9 — seven prior rotations all on axis A, and this one too, unsettled.
mk_log "$TMP/rot.jsonl" 7200 12 '"A"' '"A"' '"A"' '"A"' '"A"' '"A"' '"A"' '"A"'
mk_handoff "$GOOD_AXIS" "$GOOD_CLOSED" "$GOOD_GATE"
run_case "9 axis skew unsettled" 1 'TRIG-5 FAIL'

# 10 — same skew, settled in writing.
mk_handoff "$GOOD_AXIS" "$GOOD_CLOSED" "$GOOD_GATE" \
  "axes-review: B at 0.5096 untouched since 08-30; E still 0%; C closed; D framing only"
run_case "10 axis skew settled" 0 'TRIG-5 PASS'

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
