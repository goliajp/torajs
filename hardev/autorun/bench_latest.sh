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
# One run is not a verdict. Consecutive full-matrix runs of the same tree
# on the same machine moved the median by ±0.02 and carried four cells
# across the 0.33 line (2026-09-23: r2 0.4362 · r3 0.4173 · r4 0.4410),
# so a single file can stamp a number but never a met target. Given
# several files, each cell's ratio is the median across them, every
# derived number reads that pooled table, and `targetMet` needs at least
# three files whose own medians are all at or under the target.
#
# usage:
#   bench_latest.sh [results.json ...]   # default: newest under bench/results/
#
# exit: 0 stamp written · 2 no usable results file / files that disagree on host or bun

set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$PROJECT_DIR" || exit 2

if [ "$#" -eq 0 ]; then
  newest=$(ls -t bench/results/*.json 2>/dev/null | head -1)
  [ -n "$newest" ] || { echo "bench_latest: no results file" >&2; exit 2; }
  set -- "$newest"
fi
for f in "$@"; do
  [ -f "$f" ] || { echo "bench_latest: no results file $f" >&2; exit 2; }
done

OUT="${HARDEV_BENCH_JSON:-$PROJECT_DIR/hardev/bench-latest.json}"

python3 - "$OUT" "${HARDEV_BENCH_TARGET:-0.33}" "$@" <<'PY' || exit 2
import json, statistics, sys, datetime, os

out, target, srcs = sys.argv[1], float(sys.argv[2]), sys.argv[3:]
docs = [json.load(open(p)) for p in srcs]


def cells_of(doc):
    by = {}
    for r in doc.get("rows", []):
        if r.get("status") == "ok":
            by.setdefault(r["case"], {})[r["runtime"]] = r
    return by


def bun_of(doc):
    return next((r.get("runtime_version") for r in doc.get("rows", [])
                 if r.get("runtime") == "bun-aot"), None)


tables = [cells_of(d) for d in docs]
for label, key in (("host", lambda d: d.get("host")), ("bun", bun_of)):
    seen = {key(d) for d in docs}
    if len(seen) > 1:
        print("bench_latest: files disagree on %s: %s" % (label, sorted(map(str, seen))),
              file=sys.stderr)
        sys.exit(2)


def per_file(by, mine, theirs, field):
    out = {}
    for case, cells in by.items():
        a, b = cells.get(mine), cells.get(theirs)
        if a and b and a.get(field) and b.get(field):
            out[case] = a[field] / b[field]
    return out


def ratios(mine, theirs, field="run_ms"):
    # Pooled: a cell's ratio is its median across the files that have it.
    pooled = {}
    for by in tables:
        for case, v in per_file(by, mine, theirs, field).items():
            pooled.setdefault(case, []).append(v)
    return sorted(statistics.median(vs) for vs in pooled.values())


main = ratios("torajs", "bun-aot")
file_medians = []
for p, by in zip(srcs, tables):
    vals = list(per_file(by, "torajs", "bun-aot", "run_ms").values())
    if not vals:
        print("bench_latest: no torajs/bun-aot pairs in " + p, file=sys.stderr)
        sys.exit(2)
    file_medians.append(round(statistics.median(vals), 4))

doc = max(docs, key=lambda d: d.get("started_at") or "")
src = srcs[docs.index(doc)]
runs = min((d.get("runs") or 0) for d in docs)
stamp = {
    "tool": "bench",
    "ranAt": datetime.datetime.now(datetime.timezone.utc)
                 .strftime("%Y-%m-%dT%H:%M:%SZ"),
    # The harness stamps the sha of the tree it ran on, and that tree
    # is mini's — which takes source by rsync while its git HEAD stays
    # behind, so the recorded sha names a commit the run did not
    # measure (`…-dirty` on top of a sha from days ago). The test262
    # sweep grew a `--head-sha` for exactly this in rotation 598; this
    # side kept stamping the wrong one. HARDEV_HEAD_SHA is the dev-side
    # answer, and `headShaSource` says which one a reader is holding.
    "headSha": os.environ.get("HARDEV_HEAD_SHA") or doc.get("git_sha"),
    "headShaSource": "env" if os.environ.get("HARDEV_HEAD_SHA") else "results-file",
    "verdict": "ok",
    "source": src,
    "benchRanAt": doc.get("started_at"),
    "host": doc.get("host"),
    "sources": srcs,
    "runs": runs,
    "files": len(srcs),
    "fileMedians": file_medians,
    # A single run cannot separate a change from the machine, and neither
    # can a single file of runs. Recorded so a reader never has to ask,
    # and so a 1-file number is never quietly used as a baseline.
    "lowConfidence": (runs or 0) < 3 or len(srcs) < 3,
    "targetMet": len(srcs) >= 3 and max(file_medians) <= target,
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
    "bunStamped": bun_of(doc),
}
# The rust row is only a native-ceiling reading where main.rs does the
# same work (bench/rust-semantics.json). Carried as a second median next
# to the all-cells one, never instead of it.
sem = json.load(open("bench/rust-semantics.json"))
rust_cases = set()
for by in tables:
    rust_cases |= set(per_file(by, "torajs", "rust", "run_ms"))
unlisted = sorted(c for c in rust_cases if c not in sem)
if unlisted:
    print("bench_latest: rust rows with no entry in bench/rust-semantics.json: "
          + ", ".join(unlisted), file=sys.stderr)
    sys.exit(2)
same = {c for c, v in sem.items() if isinstance(v, dict) and v.get("kind") == "same"}
pooled_same = {}
for by in tables:
    for case, v in per_file(by, "torajs", "rust", "run_ms").items():
        if case in same:
            pooled_same.setdefault(case, []).append(v)
if pooled_same:
    stamp["medianVsRustSameProgram"] = round(statistics.median(
        statistics.median(vs) for vs in pooled_same.values()), 4)
    stamp["rustSameProgramCells"] = len(pooled_same)
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
      "runs={runs} files={files} met={met} lowConfidence={lc} bun={bunStamped} "
      "ran={benchRanAt}".format(
          lc=str(stamp["lowConfidence"]).lower(),
          met=str(stamp["targetMet"]).lower(), **stamp))
PY
