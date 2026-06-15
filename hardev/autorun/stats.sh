#!/usr/bin/env bash
#
# hardev autorun pillar — rotation log statistical baseline.
#
# Reads hardev/autorun/rotations.jsonl, emits markdown-formatted
# distribution of:
#   - self→self interval (session wall time)
#   - commits per session (git rev-list <prev prevHead>..<this prevHead>)
#   - recent-N tail with both metrics inline
#
# Used by P0 "measure-before-mechanism" of TRIG-1..4 (rotation trigger
# gate). Pure read; never modifies state.
#
# Usage:
#   hardev/autorun/stats.sh                  # default: torajs, all rows
#   hardev/autorun/stats.sh --tail 20        # recent 20 self rotations
#
# Zero deps beyond python3 + git.

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/lib.sh"

PROJECT="${HARDEV_AUTORUN_PROJECT:-torajs}"
TAIL_N="10"

while [ $# -gt 0 ]; do
  case "$1" in
    --tail) TAIL_N="${2:?--tail needs N}"; shift 2 ;;
    --project) PROJECT="${2:?--project needs name}"; shift 2 ;;
    *) echo "stats.sh: unknown arg '$1'" >&2; exit 2 ;;
  esac
done

if [ ! -f "$ROTATIONS_LOG" ]; then
  echo "stats.sh: $ROTATIONS_LOG not found" >&2
  exit 2
fi

python3 - "$ROTATIONS_LOG" "$PROJECT_DIR" "$PROJECT" "$TAIL_N" <<'PY'
import json, subprocess, sys

log, project_dir, project, tail_n = sys.argv[1:5]
tail_n = int(tail_n)

with open(log) as f:
    rows = [json.loads(line) for line in f if line.strip()]
rows = [r for r in rows if r.get("project") == project]
rows.sort(key=lambda r: r["ts"])

self_rows = [r for r in rows if r["trigger"] == "self"]
manual_rows = [r for r in rows if r["trigger"] == "manual"]


def commit_count(prev_head, this_head):
    if not prev_head or not this_head or prev_head == this_head:
        return 0
    try:
        out = subprocess.check_output(
            ["git", "-C", project_dir, "rev-list", "--count",
             f"{prev_head}..{this_head}"],
            stderr=subprocess.DEVNULL,
        )
        return int(out.decode().strip())
    except Exception:
        return None


intervals = []  # (sec, commits, curr_row)
for prev, curr in zip(self_rows[:-1], self_rows[1:]):
    sec = curr["ts"] - prev["ts"]
    if sec <= 0:
        continue
    c = commit_count(prev["prevHead"], curr["prevHead"])
    intervals.append((sec, c, curr))


def stats(xs):
    if not xs:
        return None
    s = sorted(xs)
    n = len(s)
    pick = lambda q: s[min(int(n * q), n - 1)]
    return {
        "n": n,
        "min": s[0],
        "p25": pick(0.25),
        "p50": pick(0.50),
        "p75": pick(0.75),
        "p90": pick(0.90),
        "max": s[-1],
    }


def fmt_min(s):
    return f"{s / 60:.1f} min"


durations = [d for d, _, _ in intervals]
commits = [c for _, c, _ in intervals if c is not None]

print(f"# autorun rotation baseline · project={project}")
print()
print(f"- total rotations: {len(rows)} (self={len(self_rows)}, manual={len(manual_rows)})")
if rows:
    print(f"- range: {rows[0]['at']} → {rows[-1]['at']}")
print(f"- self→self interval samples: {len(durations)}")
print()

print("## self→self session wall time")
d = stats(durations)
if d:
    print(f"- n={d['n']}")
    print(f"- min  {d['min']:>6} s ({fmt_min(d['min'])})")
    print(f"- p25  {d['p25']:>6} s ({fmt_min(d['p25'])})")
    print(f"- p50  {d['p50']:>6} s ({fmt_min(d['p50'])})")
    print(f"- p75  {d['p75']:>6} s ({fmt_min(d['p75'])})")
    print(f"- p90  {d['p90']:>6} s ({fmt_min(d['p90'])})")
    print(f"- max  {d['max']:>6} s ({fmt_min(d['max'])})")
print()

print("## commits per self→self session (rev-list count)")
c = stats(commits)
if c:
    print(f"- n={c['n']}")
    print(f"- min={c['min']} · p25={c['p25']} · p50={c['p50']} · p75={c['p75']} · p90={c['p90']} · max={c['max']}")
print()

print(f"## recent {tail_n} self→self sessions (tail)")
print()
print("| # | sec | min | commits | prevHead |")
print("|---|-----|-----|---------|----------|")
recent = intervals[-tail_n:]
base_idx = len(intervals) - len(recent)
for i, (sec, cn, row) in enumerate(recent):
    cn_s = str(cn) if cn is not None else "-"
    print(f"| {base_idx + i + 1} | {sec} | {sec/60:.1f} | {cn_s} | {row['prevHead'][:10]} |")
print()

print("## TRIG threshold (takagi 2026-06-15 framing — '1/2-3/4 ctx 至少')")
print()
N = 5
M = 5400
print(f"- TRIG-1 N (commit count): {N}")
print(f"- TRIG-2 M (wall seconds): {M} ({M/60:.0f} min)")
print()

if commits and durations:
    p_pass_n = sum(1 for x in commits if x >= N) / len(commits) * 100
    p_pass_m = sum(1 for x in durations if x >= M) / len(durations) * 100
    p_pass_both = sum(
        1 for d, c, _ in intervals
        if (c is not None and c >= N) and d >= M
    ) / len(intervals) * 100
    print(f"- % historical self rotations passing TRIG-1 (≥{N} commits): {p_pass_n:.1f}%")
    print(f"- % historical self rotations passing TRIG-2 (≥{M}s): {p_pass_m:.1f}%")
    print(f"- % historical self rotations passing BOTH: {p_pass_both:.1f}%")
print()

# Recent-30 vs all-time drift surface
if len(intervals) >= 30:
    recent30 = intervals[-30:]
    rd = [d for d, _, _ in recent30]
    rc = [c for _, c, _ in recent30 if c is not None]
    print("## recent 30 vs all-time (drift surface)")
    print()
    all_d, all_c = stats(durations), stats(commits)
    r_d, r_c = stats(rd), stats(rc)
    print(f"- wall p50 — all={all_d['p50']}s ({fmt_min(all_d['p50'])}) · recent30={r_d['p50']}s ({fmt_min(r_d['p50'])})")
    print(f"- wall p75 — all={all_d['p75']}s ({fmt_min(all_d['p75'])}) · recent30={r_d['p75']}s ({fmt_min(r_d['p75'])})")
    print(f"- commits p50 — all={all_c['p50']} · recent30={r_c['p50']}")
    print(f"- commits p75 — all={all_c['p75']} · recent30={r_c['p75']}")
PY
