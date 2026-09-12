#!/usr/bin/env bash
#
# hardev autorun pillar — reduce the newest bench run to a committed stamp.
#
# Why this exists. test262 has hardev/test262-latest.json: one committed
# file, read by the gate, drawn by the dashboard, and moving in plain
# sight every rotation. Axis B had nothing of the kind — 382 loose files
# under bench/results/ and no latest — so its number was only ever
# recomputed when somebody asked for a report. It was asked for on
# 2026-09-09, and the answer was that the full-matrix median had gone
#
#   0.500 (07-20) · 0.513 (08-18) · 0.502 (08-29) · 0.507 (09-08)
#
# — fifty days inside ±3% of where it started, against a target of 0.33.
# Every rotation in that window wrote "bench 本轮未跑" in its axes-review
# and every one of those sentences was individually true. What was
# missing was the number standing next to them.
#
# The ratio, not the wall clock. Machine state drifts 4-9% within minutes
# (rotation 211: a 4.5% "regression" that dissolved when the unmodified
# bun-aot control turned out to have moved 8.7% the same way), so the
# self-normalising tr/competitor ratio inside one run is the judgement,
# and raw run_ms is only ever supporting evidence.
#
# usage:
#   bench_latest.sh [results.json]     # default: newest under bench/results/
#
# exit: 0 stamp written · 2 no usable results file

set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$PROJECT_DIR" || exit 2

SRC="${1:-}"
if [ -z "$SRC" ]; then
  SRC=$(ls -t bench/results/*.json 2>/dev/null | head -1)
fi
[ -n "$SRC" ] && [ -f "$SRC" ] || { echo "bench_latest: no results file" >&2; exit 2; }

OUT="${HARDEV_BENCH_JSON:-$PROJECT_DIR/hardev/bench-latest.json}"

python3 - "$SRC" "$OUT" "${HARDEV_BENCH_TARGET:-0.33}" <<'PY' || exit 2
import json, statistics, sys, datetime, os

src, out, target = sys.argv[1], sys.argv[2], float(sys.argv[3])
doc = json.load(open(src))
by = {}
for r in doc.get("rows", []):
    if r.get("status") == "ok":
        by.setdefault(r["case"], {})[r["runtime"]] = r


def ratios(mine, theirs, field="run_ms"):
    out = []
    for cells in by.values():
        a, b = cells.get(mine), cells.get(theirs)
        if a and b and a.get(field) and b.get(field):
            out.append(a[field] / b[field])
    return sorted(out)


main = ratios("torajs", "bun-aot")
if not main:
    print("bench_latest: no torajs/bun-aot pairs in " + src, file=sys.stderr)
    sys.exit(2)

runs = doc.get("runs")
stamp = {
    "tool": "bench",
    "ranAt": datetime.datetime.now(datetime.timezone.utc)
                 .strftime("%Y-%m-%dT%H:%M:%SZ"),
    "headSha": doc.get("git_sha"),
    "headShaSource": "results-file",
    "verdict": "ok",
    "source": src,
    "benchRanAt": doc.get("started_at"),
    "host": doc.get("host"),
    "runs": runs,
    # A single run cannot separate a change from the machine. Recorded so
    # a reader never has to ask, and so a 1-run number is never quietly
    # used as a baseline.
    "lowConfidence": (runs or 0) < 3,
    "cells": len(main),
    "medianVsBunAot": round(statistics.median(main), 4),
    "cellsSlowerThanBunAot": sum(1 for v in main if v >= 1.0),
    "bestVsBunAot": round(main[0], 4),
    "worstVsBunAot": round(main[-1], 4),
    "target": target,
    "shortfallX": round(statistics.median(main) / target, 3),
    # The competitor's version, verbatim from the rows. bun_latest_check.sh
    # holds it against the upstream tag: a ratio measured against last
    # week's bun is a ratio against a competitor that no longer exists.
    "bunStamped": next((r.get("runtime_version") for r in doc.get("rows", [])
                        if r.get("runtime") == "bun-aot"), None),
}
for label, other in (("Node", "node-v8"), ("Go", "go"), ("Rust", "rust"),
                     ("BunJsc", "bun-jsc")):
    mine = "torajs-run" if other == "bun-jsc" else "torajs"
    vals = ratios(mine, other)
    if vals:
        stamp["medianVs" + label] = round(statistics.median(vals), 4)
# The dashboard (hardev/web/scripts/snapshot.mjs, buildBench) reduces the
# same rows the other way round: geometric mean of bun/tora, i.e. a
# speedup, for display. The gate needs median tora/bun, because that is
# the shape roadmap's axis-B predicate is written in ("full-matrix median
# tr/bun-aot <= 0.33"). Both are carried so the two readings can be
# checked against each other instead of drifting apart in two files.
import math
for label, other in (("BunAot", "bun-aot"), ("NodeV8", "node-v8")):
    vals = [v for v in ratios("torajs", other) if v > 0]
    if vals:
        stamp["geomeanSpeedupVs" + label] = round(
            math.exp(sum(math.log(1.0 / v) for v in vals) / len(vals)), 4)

art = ratios("torajs", "bun-aot", "artifact_bytes")
if art:
    stamp["artifactRatioVsBunAot"] = round(statistics.median(art), 6)

os.makedirs(os.path.dirname(out), exist_ok=True)
with open(out, "w") as f:
    json.dump(stamp, f, indent=2)
    f.write("\n")

print("bench: head={headSha} median={medianVsBunAot} target={target} "
      "shortfall={shortfallX}x cells={cells} slower={cellsSlowerThanBunAot} "
      "runs={runs} lowConfidence={lc} bun={bunStamped} ran={benchRanAt}".format(
          lc=str(stamp["lowConfidence"]).lower(), **stamp))
PY
