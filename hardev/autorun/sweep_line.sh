#!/usr/bin/env bash
#
# hardev autorun pillar — emit the handoff's sweep line from the stamp.
#
# Every rotation's step-0b report restates seven counters and their seven
# deltas. Both halves have gone wrong in ways that looked completely
# normal:
#
#   r613  the delta baseline was read from mini's copy of the verdicts
#         gz, which is not in the rsync channel and had stopped at an old
#         HEAD. The diff that came out was well-formed and plausible —
#         315 changes, one pass regression, fourteen backward — and cost
#         a full rotation of A/B to disprove. Against the real baseline
#         it was 119 changes, all forward, zero regressions.
#   r571  a count compared across two samples whose membership had
#         shifted under it.
#
# So the baseline is taken from git — the previous committed version of
# hardev/test262-latest.json, which is the only copy that cannot be
# stale in a way nobody notices — and the deltas are computed here rather
# than by hand. Conservation (ΔtrAccepted == Δpass + Δbug) is checked in
# the same pass; it is the one identity that catches a mis-paired
# baseline, and it is checked here instead of being eyeballed.
#
# usage:
#   sweep_line.sh              # one line, ready to paste into handoff.md
#
# TRIG-7 re-derives this line and compares. A hand-edited number fails
# the gate; a pasted one cannot be wrong, which is the point — the cheapest
# way to stop transcription errors is to stop transcribing.
#
# exit: 0 line emitted · 1 conservation broken · 2 no stamp to read

set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
STAMP="${HARDEV_SWEEP_JSON:-$PROJECT_DIR/hardev/test262-latest.json}"

[ -f "$STAMP" ] || { echo "sweep_line: no sweep stamp at $STAMP" >&2; exit 2; }

# The baseline is the most recent COMMITTED version whose headSha differs
# from the one in hand — not "the second-newest commit touching the file".
# Those differ exactly when the current sweep has not been committed yet,
# which is every time this is run at close: `log -2 | tail -1` then reaches
# back two rotations and reports two rotations of movement as one. The
# conservation identity does not catch it, because a two-rotation delta is
# just as self-consistent as a one-rotation delta — both are internally
# fine and only an outside fact (last rotation's reported passTotal) shows
# which one is being answered.
CUR_SHA="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1])).get("headSha") or "")' "$STAMP" 2>/dev/null)"
PREV=""
for c in $(git -C "$PROJECT_DIR" log --format=%H -- hardev/test262-latest.json 2>/dev/null | head -20); do
  h="$(git -C "$PROJECT_DIR" show "$c:hardev/test262-latest.json" 2>/dev/null \
       | python3 -c 'import json,sys;print(json.load(sys.stdin).get("headSha") or "")' 2>/dev/null)"
  if [ -n "$h" ] && [ "$h" != "$CUR_SHA" ]; then PREV="$c"; break; fi
done
# Exported, not just assigned: the python below reads it from the
# environment. Without the export it silently sees an empty baseline and
# prints seven bare counters with no deltas — which looks like a first
# run rather than like a broken one.
# HARDEV_SWEEP_BASE_JSON overrides the baseline (set-but-empty means "no
# baseline"), so the self-test can drive the deltaless branch without
# depending on this repo's real sweep history.
export PREV_JSON=""
if [ -n "${HARDEV_SWEEP_BASE_JSON+set}" ]; then
  PREV_JSON="$HARDEV_SWEEP_BASE_JSON"
elif [ -n "$PREV" ]; then
  PREV_JSON="$(git -C "$PROJECT_DIR" show "$PREV:hardev/test262-latest.json" 2>/dev/null || true)"
fi

python3 - "$STAMP" <<'PY'
import json, os, sys

cur = json.load(open(sys.argv[1]))
prev_raw = os.environ.get("PREV_JSON") or ""
try:
    prev = json.loads(prev_raw) if prev_raw.strip() else None
except json.JSONDecodeError:
    prev = None
# A baseline that is the same run is not a baseline. Comparing a sweep
# against itself yields seven zeroes, which reads exactly like "nothing
# moved this rotation" — the failure mode that hides real movement.
if prev and prev.get("headSha") == cur.get("headSha"):
    prev = None

KEYS = ["pass", "passNoOracle", "passNegative", "passTotal",
        "bug", "incompatible", "trAccepted"]

parts = [f"head={cur.get('headSha')}"]
for k in KEYS:
    v = cur.get(k)
    if prev is None or prev.get(k) is None:
        parts.append(f"{k}={v}")
    else:
        d = v - prev[k]
        parts.append(f"{k}={v}({d:+d})")
parts.append(f"harnessError={cur.get('harnessError')}")

ok = True
if prev is not None:
    d_acc = cur["trAccepted"] - prev["trAccepted"]
    d_pass = cur["passTotal"] - prev["passTotal"]
    d_bug = cur["bug"] - prev["bug"]
    ok = d_acc == d_pass + d_bug
    parts.append("conservation=" + ("ok" if ok else
                 f"BROKEN({d_acc}!={d_pass}+{d_bug})"))
    parts.append(f"base={prev.get('headSha')}")
else:
    parts.append("conservation=n/a")

print("sweep: " + " ".join(parts))
if not ok:
    print("sweep_line: ΔtrAccepted != Δpass + Δbug — the baseline is not the "
          "run this sweep followed, or a bucket moved without being counted",
          file=sys.stderr)
    sys.exit(1)
PY
