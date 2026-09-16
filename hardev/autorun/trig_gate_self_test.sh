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
#  11  a stamp missing                    → exit 1, TRIG-6 FAIL
#  12  a stamp from an earlier session    → exit 1, TRIG-6 FAIL
#  13  a stamp written against a dirty tree → exit 1, TRIG-6 FAIL
#  14  a stamp whose verdict is fail      → exit 1, TRIG-6 FAIL
#  15  no sweep: line in the handoff       → exit 1, TRIG-7 FAIL
#  16  a hand-edited sweep counter         → exit 1, TRIG-7 FAIL
#  17  the pasted sweep line               → exit 0, TRIG-7 PASS
#  18  axis skew, review without the reading → exit 1, TRIG-5 FAIL
#  19  axis skew, axis-B reading months old  → exit 1, TRIG-5 FAIL
#  20  commits enough, one thing closed     → exit 1, TRIG-1 FAIL
#  23  every commit carries Agent-Origin     → exit 1, TRIG-1 FAIL
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
REAL_SHA2="$(git -C "$PROJECT_DIR" rev-parse --short HEAD~1)"
REAL_SHA3="$(git -C "$PROJECT_DIR" rev-parse --short HEAD~2)"
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

# Four run stamps, all naming HEAD, all green — the shape TRIG-6 wants.
# Every case below inherits these unless it deliberately breaks one.
mk_stamps() {
  mkdir -p "$TMP/stamps"
  local n
  for n in determinism gmalloc file-size; do
    printf '{"tool":"%s","headSha":"%s","verdict":"ok"}\n' \
      "$n" "$REAL_SHA" > "$TMP/stamps/$n-latest.json"
  done
  # The sweep stamp carries real counters: TRIG-7 re-derives its line
  # from them. HARDEV_SWEEP_BASE_JSON is set-but-empty in run_case, so
  # the deltas are suppressed and the fixture needs no baseline.
  # A bench stamp dated now: TRIG-5 rejects an axis-B reading older than 14d.
  # bunStamped equals the pinned HARDEV_BUN_UPSTREAM (set in run_case) so
  # the fixture is "current" without asking GitHub; case 21 breaks that.
  printf '{"tool":"bench","headSha":"%s","benchRanAt":"%s","runs":3,'\
'"lowConfidence":false,"cells":46,"medianVsBunAot":0.5067,'\
'"cellsSlowerThanBunAot":1,"target":0.33,"shortfallX":1.535,"bunStamped":"1.4.2"}\n' \
    "$REAL_SHA" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$TMP/stamps/bench-latest.json"
  BENCH_LINE=$(HARDEV_BENCH_JSON="$TMP/stamps/bench-latest.json" \
    bash "$SCRIPT_DIR/bench_latest_line.sh")
  printf '{"tool":"test262","headSha":"%s","harnessError":0,"pass":31277,'\
'"passNoOracle":1186,"passNegative":4511,"passTotal":36974,"bug":11555,'\
'"incompatible":4645,"trAccepted":48529}\n' "$REAL_SHA" \
    > "$TMP/stamps/test262-latest.json"
}

# The sweep line as sweep_line.sh renders the fixture above.
SWEEP_LINE="sweep: head=REPLACED pass=31277 passNoOracle=1186 passNegative=4511 passTotal=36974 bug=11555 incompatible=4645 trAccepted=48529 harnessError=0 conservation=n/a"

mk_handoff() {
  printf '%s\n' "## rotate 触发" "" "$@" > "$TMP/handoff.md"
  # TRIG-7 reads the whole handoff, so every fixture carries the sweep
  # line unless the case is specifically about it.
  printf '%s\n' "${SWEEP_LINE/REPLACED/$REAL_SHA}" >> "$TMP/handoff.md"
}

run_case() {
  local name=$1 want_exit=$2 want_line=$3
  local out rc
  out=$(HARDEV_ROTATIONS_LOG="$TMP/rot.jsonl" HARDEV_HANDOFF_FILE="$TMP/handoff.md" \
        HARDEV_STAMP_DIR="$TMP/stamps" HARDEV_SWEEP_BASE_JSON= \
        HARDEV_BUN_UPSTREAM=1.4.2 \
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
# Three entries: TRIG-1's second measure wants >= 3 things named, and a
# fixture that only ever carries one would make the happy path depend on
# the threshold never rising.
GOOD_CLOSED="closed: $REAL_SHA did a thing; $REAL_SHA2 did another; $REAL_SHA3 did a third"
GOOD_GATE="gate: 3773/0/4"

mk_stamps

# 1 — happy path: 12 commits, 2h wall, full section, four green stamps.
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
  "axes-review: E still 0%; C closed; D framing only. $BENCH_LINE"
run_case "10 axis skew settled" 0 'TRIG-5 PASS'

# ── TRIG-6 — the stamps ───────────────────────────────────────────────
mk_log "$TMP/rot.jsonl" 7200 12
mk_handoff "$GOOD_AXIS" "$GOOD_CLOSED" "$GOOD_GATE"

rm -f "$TMP/stamps/gmalloc-latest.json"
run_case "11 stamp missing" 1 'TRIG-6 FAIL.*missing: gmalloc'
mk_stamps

# The carry-forward this gate exists to stop: last rotation's answer,
# correct in itself, wearing this rotation's label.
printf '{"tool":"determinism","headSha":"%s","verdict":"ok"}\n' "$OLD_SHA" \
  > "$TMP/stamps/determinism-latest.json"
run_case "12 stamp from earlier session" 1 'TRIG-6 FAIL.*not from this session'
mk_stamps

# A dirty tree measures something that is not any commit.
printf '{"tool":"file_size_audit","headSha":"%s-dirty","verdict":"ok"}\n' "$REAL_SHA" \
  > "$TMP/stamps/file-size-latest.json"
run_case "13 stamp from a dirty tree" 1 'TRIG-6 FAIL.*not from this session'
mk_stamps

printf '{"tool":"gmalloc_scan","headSha":"%s","verdict":"fail"}\n' "$REAL_SHA" \
  > "$TMP/stamps/gmalloc-latest.json"
run_case "14 stamp verdict red" 1 'TRIG-6 FAIL.*red: gmalloc'
mk_stamps

# ── TRIG-7 — the sweep line ───────────────────────────────────────────
mk_log "$TMP/rot.jsonl" 7200 12

printf '%s\n' "## rotate 触发" "" "$GOOD_AXIS" "$GOOD_CLOSED" "$GOOD_GATE" > "$TMP/handoff.md"
run_case "15 no sweep line" 1 "TRIG-7 FAIL.*no 'sweep:' line"

mk_handoff "$GOOD_AXIS" "$GOOD_CLOSED" "$GOOD_GATE"
sed -i '' 's/passTotal=36974/passTotal=36999/' "$TMP/handoff.md"
run_case "16 hand-edited counter" 1 'TRIG-7 FAIL.*disagrees.*passTotal=36974'

mk_handoff "$GOOD_AXIS" "$GOOD_CLOSED" "$GOOD_GATE"
run_case "17 pasted sweep line" 0 'TRIG-7 PASS'

# ── TRIG-5 — the axis-B reading ─────────────────────────────────
# "bench 本轮未跑" is a true sentence that says nothing. The gate wants the
# number, and wants it recent.
mk_log "$TMP/rot.jsonl" 7200 12 '"A"' '"A"' '"A"' '"A"' '"A"' '"A"' '"A"' '"A"'
mk_handoff "$GOOD_AXIS" "$GOOD_CLOSED" "$GOOD_GATE" \
  "axes-review: B 本轮未跑;E still 0%; C closed; D framing only"
run_case "18 review without the reading" 1 'TRIG-5 FAIL.*lacks the axis-B reading'

python3 -c 'import json,sys;json.dump({"tool":"bench","headSha":sys.argv[1],\
"benchRanAt":"2026-06-01T00:00:00Z","runs":3,"cells":46,"medianVsBunAot":0.5067,\
"cellsSlowerThanBunAot":1,"target":0.33,"shortfallX":1.535,"lowConfidence":False,\
"bunStamped":"1.4.2"},open(sys.argv[2],"w"))' "$REAL_SHA" "$TMP/stamps/bench-latest.json"
mk_handoff "$GOOD_AXIS" "$GOOD_CLOSED" "$GOOD_GATE" "axes-review: $BENCH_LINE"
run_case "19 axis-B reading stale" 1 'TRIG-5 FAIL.*reading is [0-9]+d old'
mk_stamps

# 21 — the reading is recent and pasted, but it was taken against a bun
# one release behind upstream. A ratio against last week's competitor.
python3 -c 'import json,sys;p=sys.argv[1];d=json.load(open(p));d["bunStamped"]="1.4.1";\
json.dump(d,open(p,"w"))' "$TMP/stamps/bench-latest.json"
OLD_BUN_LINE=$(HARDEV_BENCH_JSON="$TMP/stamps/bench-latest.json" bash "$SCRIPT_DIR/bench_latest_line.sh")
mk_handoff "$GOOD_AXIS" "$GOOD_CLOSED" "$GOOD_GATE" "axes-review: $OLD_BUN_LINE"
run_case "21 bench against a stale bun" 1 'TRIG-5 FAIL.*bun is not upstream-latest.*stamped=1.4.1'
mk_stamps

# 22 — the stamp has no bun version at all: unknown is not current.
python3 -c 'import json,sys;p=sys.argv[1];d=json.load(open(p));d.pop("bunStamped");\
json.dump(d,open(p,"w"))' "$TMP/stamps/bench-latest.json"
NO_BUN_LINE=$(HARDEV_BENCH_JSON="$TMP/stamps/bench-latest.json" bash "$SCRIPT_DIR/bench_latest_line.sh")
mk_handoff "$GOOD_AXIS" "$GOOD_CLOSED" "$GOOD_GATE" "axes-review: $NO_BUN_LINE"
run_case "22 bench stamp without a bun version" 1 'TRIG-5 FAIL.*bun is not upstream-latest.*status=unknown'
mk_stamps

# 20 — the count is met by slicing thinner; the closed list is not.
mk_log "$TMP/rot.jsonl" 7200 12
mk_handoff "$GOOD_AXIS" "closed: $REAL_SHA one thing only" "$GOOD_GATE"
run_case "20 twelve commits, one thing closed" 1 "TRIG-1 FAIL.*only 1 thing"

# 23 — agent-landed commits are not the main session's work. A pattern
# that matches every message turns twelve commits into zero; case 1
# (default pattern) is the other half: nothing real carries the trailer.
mk_log "$TMP/rot.jsonl" 7200 12
mk_handoff "$GOOD_AXIS" "$GOOD_CLOSED" "$GOOD_GATE"
export HARDEV_AGENT_ORIGIN_PATTERN=.
run_case "23 agent-origin commits not counted" 1 'TRIG-1 FAIL.*commits=0 '
unset HARDEV_AGENT_ORIGIN_PATTERN

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
