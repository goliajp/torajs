#!/usr/bin/env bash
#
# hardev autorun pillar — TRIG-1..5 pre-trigger gate.
#
# Run by `trigger.sh self` before it generates a rotation_id and writes
# any state. Governs whether a self-initiated rotation is *legitimate*
# to trigger in the first place. Mirror of `check.sh` (INV-1..5) which
# governs whether an already-triggered rotation may *execute*.
#
# Spec: see `hardev/autorun/README.md` → "TRIG-1..5 spec" section.
# Threshold rationale: `hardev/metrics.md` §6 "P5 lock @2026-09-07".
#
# ── 2026-09-07 rewrite (takagi) ───────────────────────────────────────
# The four gates as shipped in P2.0 all pointed the same way — against
# rotating *too early* — so in practice only TRIG-1 ever bit, and it had
# turned into a quota rather than a floor. Measured over the last 60
# rotations before this rewrite (reconstructed from `prevHead`, since
# `commitsInSession` was hardcoded null — see lib.sh):
#
#   commits/rotation  min 5 · p10 5 · p50 6 · p90 8 · max 9   (0/60 below 5)
#   wall (min)               p10 95 · p50 125 · p90 221
#   triggerReason            d: 606 / a: 133 / c: 2 / b: 2    (81.6% = d)
#
# Left edge glued to the threshold, nothing below it, nothing far above:
# the gate had become the target. TRIG-2's floor (90 min) sat below the
# p10 (95 min), so it constrained nothing. And `reason: d` — whose own
# definition is "commit >= TRIG-1" — carried 81.6% of rotations, i.e.
# TRIG-3 was restating TRIG-1 rather than adding anything.
#
# So each gate now owns a different question:
#
#   TRIG-1  the floor      — is there enough work in this session?
#   TRIG-2  both edges     — too fast to be real / too long to stay sharp
#                            (and it is the ONLY escape hatch, see below)
#   TRIG-3  the content    — which axis, what closed, what the gate reads
#   TRIG-4  the wording    — the known procrastination phrasings
#   TRIG-5  the allocation — is one axis starving the others?
#   TRIG-6  the evidence   — were the mechanical checks actually run,
#                            on THIS source?
#   TRIG-7  the transcript — does the handoff's sweep line still say what
#                            the sweep said?
#
# TRIG-6 (2026-09-09) closes the structural hole the other five share:
# every one of them reads what the handoff *says*. TRIG-3 checks the
# `gate:` triple for F=0 and never asks whether N is a real number; the
# sweep / determinism / Guard-Malloc lines are free text. So "it was not
# run", "it ran on the previous HEAD" and "last rotation's number got
# carried forward" are all invisible — and torajs-autorun-pipeline.md
# records that class at least five times, including a sweep that stayed
# dark for five rotations while six pass regressions piled up. The four
# checks now leave committed JSON stamps carrying the sha of the source
# they measured, and the gate reads those instead.
#
# Why N went 5 -> 12: every rotation pays a fixed cost that does not
# scale with commits — sweep 507s + build_determinism 197s +
# gmalloc_scan 401s = 1105s = 18.4 min of machine time, plus writing the
# handoff and rebuilding context on the far side. At p50 (6 commits /
# 125 min) that machine floor alone is 14.7% of the rotation. Solving
# wall(N) = 18.4 + N * 17.8 from the measured p50: N=12 gives 232 min
# (7.9%), N=20 gives 374 min (4.9%). Doubling to 12 takes most of the
# recoverable overhead; going to 20 buys a third as much again while
# pushing wall past 221 min — the observed p90 — into a region no
# rotation has ever been measured in, and drift in exactly that region
# is the only reason rotations exist. 12 is the far edge of evidence,
# not a cautious pick. metrics.md §6 had always said 5/5400 were
# *candidates* to be locked after a week of measurement (P5); that lock
# never happened. This is it, and the same rule applies again: run a
# week under 12, measure, then decide whether 16-20 is earned.
#
# Why TRIG-2 inverted: with a 12-commit floor, a rotation that spends
# five hours on one hard problem (a decomposition, an E0-shaped
# refactor) can be unable to close at all. `trigger.sh manual` is
# takagi's override and is not a path the agent may take, so the escape
# has to be mechanical: past TRIG2_MAX_WALL_SEC the commit floor is
# waived. The same number doubles as "this session is long enough that
# drift now outweighs amortisation". The new floor (30 min) catches the
# opposite failure — twelve trivial commits inside half an hour.
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
# one of PASS / FAIL / SKIP / WAIVED. Lines are stable for greppability
# by trigger.sh and self-test assertions.

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
. "$SCRIPT_DIR/lib.sh"

# ── Tunables (locked 2026-09-07; see metrics.md §6 "P5 lock") ─────────
TRIG1_MIN_COMMITS="${HARDEV_TRIG1_MIN_COMMITS:-12}"
TRIG2_MIN_WALL_SEC="${HARDEV_TRIG2_MIN_WALL_SEC:-1800}"    # 30 min floor
TRIG2_MAX_WALL_SEC="${HARDEV_TRIG2_MAX_WALL_SEC:-18000}"   # 300 min cap → waives TRIG-1
TRIG5_SAME_AXIS_MAX="${HARDEV_TRIG5_SAME_AXIS_MAX:-8}"     # consecutive same-axis rotations
STAMP_DIR="${HARDEV_STAMP_DIR:-$PROJECT_DIR/hardev}"       # where the run stamps live

# Blacklist phrases (case-insensitive). Each entry = one regex; if any
# matches anywhere inside the handoff "rotate 触发" section, TRIG-4 FAILs.
# Curated from cases#rotate-as-procrastination + 2026-06-15 surface.
# Kept verbatim through the 2026-09-07 rewrite: it costs nothing and the
# disease it catches is expensive when it comes back.
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
waive_trig1=0
wall_sec=""

emit() {
  printf '%s %s %s\n' "$1" "$2" "$3"
}

# Resolve wall time first: TRIG-2's upper edge decides whether TRIG-1 is
# waived, so it must be computed before TRIG-1 is judged (output order
# stays 1..5 — each line carries its own ID, so emit order is free).
resolve_wall() {
  local row last_ts now
  row=$(autorun_last_self_row)
  [ -n "$row" ] || return
  last_ts=$(printf '%s\n' "$row" | cut -f1)
  now=$(date +%s)
  wall_sec=$(( now - last_ts ))
  if [ "$wall_sec" -ge "$TRIG2_MAX_WALL_SEC" ]; then
    waive_trig1=1
  fi
}

# The commit range for this session, as `<base>..HEAD`. Falls back to a
# 25-commit window on cold start so TRIG-3's sha check still has ground
# to stand on.
session_range() {
  local row last_head
  row=$(autorun_last_self_row)
  last_head=$(printf '%s\n' "$row" | cut -f2)
  if [ -n "$last_head" ]; then
    printf '%s..HEAD\n' "$last_head"
  else
    printf 'HEAD~25..HEAD\n'
  fi
}

# ── TRIG-1 — session commit count ≥ N ─────────────────────────────────
# Cold start (no prior self row) → SKIP. Over the wall cap → WAIVED.
check_trig1() {
  local count
  count=$(autorun_commits_since_last_self)
  if [ -z "$count" ]; then
    emit TRIG-1 SKIP "no prior self rotation (cold start)"
    return
  fi
  if [ "$count" -ge "$TRIG1_MIN_COMMITS" ]; then
    emit TRIG-1 PASS "commits=$count ≥ N=$TRIG1_MIN_COMMITS"
    return
  fi
  if [ "$waive_trig1" -eq 1 ]; then
    emit TRIG-1 WAIVED "commits=$count < N=$TRIG1_MIN_COMMITS but wall=${wall_sec}s ≥ cap=${TRIG2_MAX_WALL_SEC}s"
    return
  fi
  emit TRIG-1 FAIL "commits=$count < N=$TRIG1_MIN_COMMITS (need ≥$TRIG1_MIN_COMMITS, or wall ≥ $(( TRIG2_MAX_WALL_SEC / 60 ))min)"
  failed+=(TRIG-1)
}

# ── TRIG-2 — wall time inside [floor, cap] ────────────────────────────
# Floor: twelve commits inside half an hour is not a session, it is a
# batch of trivia. Cap: past it the rotation is long enough that drift
# outweighs amortisation, so closing is not merely allowed but signalled
# — and TRIG-1 is waived so it can actually close.
check_trig2() {
  if [ -z "$wall_sec" ]; then
    emit TRIG-2 SKIP "no prior self rotation (cold start)"
    return
  fi
  local mins=$(( wall_sec / 60 ))
  if [ "$wall_sec" -lt "$TRIG2_MIN_WALL_SEC" ]; then
    emit TRIG-2 FAIL "wall=${wall_sec}s (${mins}min) < floor=$(( TRIG2_MIN_WALL_SEC / 60 ))min"
    failed+=(TRIG-2)
    return
  fi
  if [ "$wall_sec" -ge "$TRIG2_MAX_WALL_SEC" ]; then
    emit TRIG-2 PASS "wall=${mins}min ≥ cap=$(( TRIG2_MAX_WALL_SEC / 60 ))min — close now, TRIG-1 waived"
    return
  fi
  emit TRIG-2 PASS "wall=${mins}min inside [$(( TRIG2_MIN_WALL_SEC / 60 )), $(( TRIG2_MAX_WALL_SEC / 60 ))]min"
}

# ── TRIG-3 — the section carries content, not a restatement of TRIG-1 ─
# Requires three lines inside `## rotate 触发`:
#   axis:   <A|B|C|D|E>[,...]      which v1.0 gate axis this rotation served
#   closed: <sha> <what>           at least one sha from THIS session's range
#   gate:   <N>/<F>/<S>            conformance triple, F must be 0
# The sha requirement is the part that cannot be satisfied by wording:
# a rotation with nothing to point at cannot name a commit of its own.
check_trig3() {
  if [ ! -f "$HANDOFF_FILE" ]; then
    emit TRIG-3 FAIL "handoff.md missing at $HANDOFF_FILE"
    failed+=(TRIG-3)
    return
  fi
  local body axis closed_line gate_line
  body=$(autorun_handoff_trigger_section)
  if [ -z "$body" ]; then
    emit TRIG-3 FAIL "handoff missing '## rotate 触发' section"
    failed+=(TRIG-3)
    return
  fi

  axis=$(autorun_trigger_axis)
  if [ -z "$axis" ]; then
    emit TRIG-3 FAIL "no valid 'axis: <A|B|C|D|E>[,...]' line"
    failed+=(TRIG-3)
    return
  fi

  closed_line=$(printf '%s\n' "$body" | grep -iE '^[[:space:]]*closed:' | head -1)
  if [ -z "$closed_line" ]; then
    emit TRIG-3 FAIL "no 'closed: <sha> <what>' line (axis=$axis)"
    failed+=(TRIG-3)
    return
  fi

  # At least one hex token on the closed line must name a commit inside
  # this session's range.
  local range shas tok hit=""
  range=$(session_range)
  shas=$(git -C "$PROJECT_DIR" rev-list "$range" 2>/dev/null)
  if [ -z "$shas" ]; then
    emit TRIG-3 FAIL "git rev-list $range returned nothing — cannot verify 'closed:'"
    failed+=(TRIG-3)
    return
  fi
  for tok in $(printf '%s\n' "$closed_line" | grep -oE '\b[0-9a-f]{7,40}\b'); do
    if printf '%s\n' "$shas" | grep -q "^$tok"; then
      hit="$tok"
      break
    fi
  done
  if [ -z "$hit" ]; then
    emit TRIG-3 FAIL "no sha on the 'closed:' line belongs to $range"
    failed+=(TRIG-3)
    return
  fi

  gate_line=$(printf '%s\n' "$body" | grep -oE '^[[:space:]]*gate:[[:space:]]*[0-9]+/[0-9]+/[0-9]+' | head -1)
  if [ -z "$gate_line" ]; then
    emit TRIG-3 FAIL "no 'gate: <N>/<F>/<S>' line (axis=$axis, closed=$hit)"
    failed+=(TRIG-3)
    return
  fi
  local triple fails
  triple=$(printf '%s\n' "$gate_line" | grep -oE '[0-9]+/[0-9]+/[0-9]+')
  fails=$(printf '%s\n' "$triple" | cut -d/ -f2)
  if [ "$fails" -ne 0 ]; then
    emit TRIG-3 FAIL "gate=$triple has $fails fail — a rotation does not close red"
    failed+=(TRIG-3)
    return
  fi
  emit TRIG-3 PASS "axis=$axis closed=$hit gate=$triple"
}

# ── TRIG-4 — blacklist phrase guard ────────────────────────────────────
check_trig4() {
  if [ ! -f "$HANDOFF_FILE" ]; then
    emit TRIG-4 FAIL "handoff.md missing at $HANDOFF_FILE"
    failed+=(TRIG-4)
    return
  fi
  local body hits=()
  body=$(autorun_handoff_trigger_section)
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

# ── TRIG-5 — axis skew must be settled, not silent ────────────────────
# If the previous TRIG5_SAME_AXIS_MAX-1 self rotations all served the
# same axis as this one, the handoff must carry an `axes-review:` line —
# a written account of where the other four axes stand and why they stay
# untouched for another rotation.
#
# It does NOT force an axis switch. Which axis to work is takagi's call
# (CLAUDE.md's sequential-plan HARD RULE), and this gate has no business
# overriding it. What it removes is the silence: through September every
# rotation served axis A while axis B sat still for a month and axis E
# had never been started, and that only surfaced when a full report was
# asked for. Under this gate it surfaces every eight rotations, in
# writing, whether or not anyone asks.
check_trig5() {
  local axis
  axis=$(autorun_trigger_axis)
  if [ -z "$axis" ]; then
    emit TRIG-5 SKIP "no axis on this rotation (TRIG-3 already FAIL)"
    return
  fi
  local need=$(( TRIG5_SAME_AXIS_MAX - 1 ))
  local prior
  prior=$(python3 - "$ROTATIONS_LOG" "$(autorun_project_name)" "$need" <<'PY'
import json, sys
log, project, need = sys.argv[1], sys.argv[2], int(sys.argv[3])
rows = []
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
            if r.get("project") != project or r.get("trigger") != "self":
                continue
            rows.append(r)
except FileNotFoundError:
    pass
tail = rows[-need:] if need > 0 else []
# Rows written before the axis field existed carry None; an unknown axis
# breaks the streak rather than extending it — the gate needs `need`
# rows of real evidence before it can fire.
print(",".join((r.get("axis") or "?") for r in tail))
PY
)
  if [ -z "$prior" ]; then
    emit TRIG-5 PASS "no prior axis history yet"
    return
  fi
  local same=1 a
  local IFS=,
  for a in $prior; do
    if [ "$a" != "$axis" ]; then
      same=0
      break
    fi
  done
  unset IFS
  if [ "$same" -eq 0 ]; then
    emit TRIG-5 PASS "axis=$axis; prior $need = [$prior] not all the same"
    return
  fi
  if autorun_handoff_trigger_section | grep -iqE '^[[:space:]]*axes-review:'; then
    emit TRIG-5 PASS "axis=$axis for $TRIG5_SAME_AXIS_MAX consecutive rotations; axes-review present"
  else
    emit TRIG-5 FAIL "axis=$axis for $TRIG5_SAME_AXIS_MAX consecutive rotations — handoff needs an 'axes-review:' line stating where the other axes stand and why they wait"
    failed+=(TRIG-5)
  fi
}

# ── TRIG-6 — the mechanical checks ran, on this session's source ───────
# Four stamps, same shape as hardev/test262-latest.json (which already
# worked this way and is reused as the sweep's stamp):
#
#   test262-latest.json      the full sweep          (0b)
#   determinism-latest.json  build determinism       (0c)
#   gmalloc-latest.json      Guard Malloc scan       (0c-2)
#   file-size-latest.json    file / function sizes   (pre-flight)
#
# A stamp counts only when its headSha is a commit in THIS session and
# the tree was clean when it was written. A `-dirty` stamp measured
# something that is not any commit, and a sha from an earlier session is
# last rotation's answer wearing this rotation's label — the exact
# carry-forward this gate exists to stop.
check_trig6() {
  local range shas missing=() stale=() red=() fresh=()
  range=$(session_range)
  shas=$(git -C "$PROJECT_DIR" rev-list "$range" 2>/dev/null)
  if [ -z "$shas" ]; then
    emit TRIG-6 SKIP "git rev-list $range returned nothing"
    return
  fi
  local name file sha verdict
  for name in test262 determinism gmalloc file-size; do
    file="$STAMP_DIR/$name-latest.json"
    if [ ! -f "$file" ]; then
      missing+=("$name")
      continue
    fi
    sha=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1])).get("headSha") or "")' "$file" 2>/dev/null)
    verdict=$(python3 -c 'import json,sys;d=json.load(open(sys.argv[1]));print(d.get("verdict") or ("ok" if d.get("harnessError",0)==0 else "fail"))' "$file" 2>/dev/null)
    if [ -z "$sha" ] || [ "${sha%-dirty}" != "$sha" ]; then
      stale+=("$name(${sha:-no-sha})")
    elif ! printf '%s\n' "$shas" | grep -q "^$sha"; then
      stale+=("$name($sha)")
    elif [ "$verdict" != "ok" ]; then
      red+=("$name($verdict)")
    else
      fresh+=("$name=$sha")
    fi
  done
  if [ "${#missing[@]}" -gt 0 ] || [ "${#stale[@]}" -gt 0 ] || [ "${#red[@]}" -gt 0 ]; then
    local msg=""
    [ "${#missing[@]}" -gt 0 ] && msg="missing: ${missing[*]}; "
    [ "${#stale[@]}" -gt 0 ] && msg="${msg}not from this session: ${stale[*]}; "
    [ "${#red[@]}" -gt 0 ] && msg="${msg}red: ${red[*]}; "
    emit TRIG-6 FAIL "${msg}run them on the final HEAD and commit the stamps"
    failed+=(TRIG-6)
    return
  fi
  emit TRIG-6 PASS "4/4 stamps from this session (${fresh[*]})"
}

# ── TRIG-7 — the handoff's sweep line matches the sweep ────────────────
# Step 0b restates seven counters and seven deltas every rotation. Both
# halves have gone wrong while looking entirely normal — r613 computed
# the deltas against mini's stale copy of the baseline and produced a
# plausible, well-formed, completely false diff that took a rotation of
# A/B to disprove.
#
# `sweep_line.sh` derives the line from the committed stamp, with the
# baseline taken from git. This gate re-derives it and requires every
# token to appear in the handoff. A pasted line cannot disagree; a
# hand-edited one does. The cheapest way to stop transcription errors is
# to stop transcribing, and this is the half that makes sure it happened.
check_trig7() {
  if [ ! -f "$HANDOFF_FILE" ]; then
    emit TRIG-7 FAIL "handoff.md missing at $HANDOFF_FILE"
    failed+=(TRIG-7)
    return
  fi
  local want rc
  want=$(HARDEV_SWEEP_JSON="$STAMP_DIR/test262-latest.json" \
         bash "$SCRIPT_DIR/sweep_line.sh" 2>/dev/null)
  rc=$?
  if [ "$rc" -eq 2 ] || [ -z "$want" ]; then
    emit TRIG-7 SKIP "no sweep stamp to compare against (TRIG-6 already FAIL)"
    return
  fi
  if [ "$rc" -ne 0 ]; then
    emit TRIG-7 FAIL "conservation broken in the sweep itself: ${want#sweep: }"
    failed+=(TRIG-7)
    return
  fi
  # Compare token sets, not the literal line: handoff.md is prose and the
  # line gets wrapped. Every token the script produces must be present.
  local handoff_body missing=() tok
  handoff_body=$(tr '\n' ' ' < "$HANDOFF_FILE")
  case "$handoff_body" in
    *"sweep:"*) ;;
    *) emit TRIG-7 FAIL "handoff carries no 'sweep:' line — paste the output of hardev/autorun/sweep_line.sh"
       failed+=(TRIG-7); return ;;
  esac
  for tok in ${want#sweep: }; do
    case "$handoff_body" in
      *"$tok"*) ;;
      *) missing+=("$tok") ;;
    esac
  done
  if [ "${#missing[@]}" -gt 0 ]; then
    emit TRIG-7 FAIL "handoff sweep line disagrees with the stamp on: ${missing[*]}"
    failed+=(TRIG-7)
    return
  fi
  emit TRIG-7 PASS "sweep line matches the stamp (${want#sweep: head=})"
}

resolve_wall
check_trig1
check_trig2
check_trig3
check_trig4
check_trig5
check_trig6
check_trig7

if [ "${#failed[@]}" -gt 0 ]; then
  echo "TRIG-FAILED: ${failed[*]}" >&2
  exit 1
fi
exit 0
