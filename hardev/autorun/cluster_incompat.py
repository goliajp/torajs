#!/usr/bin/env python3
"""Cluster the test262 `incompatible` bucket, and print the v1.0 gate
predicate.

Input: the ndjson produced by `torajs-test262 --incompat-ndjson`, one
`{"path","kind","msg"}` object per line.

Two things come out of this:

1. **The census.** Of the ~39k cases the checker rejects, how much mass
   sits behind a small number of substrate gaps? The runner's stdout
   breakdown only has six `incompat_kind` prefixes, too coarse to tell
   "one missing feature blocking 4000 cases" from "4000 distinct one-off
   rejects". Clusters carry two axes: size, and how many test262 feature
   directories they span — a big cluster inside one directory is a
   feature gap, the same size across 50 directories is a cross-cutting
   substrate gap and worth far more.

2. **The gate number.** `docs/roadmap.md` defines the v1.0 gate as a
   substrate predicate, not a pass percentage: *no cluster of 4 or more
   cases outside a documented post-v1.0 phase*. The count of such
   clusters is therefore the single number for "how far is v1.0", it has
   a defined end (zero), and it is computed here rather than eyeballed.

   Expect it to rise before it falls. Unlocking a gap lets previously
   unreachable cases run, and they surface their own signatures — that
   is progress presenting as a bigger number, which is exactly why the
   raw case count is reported next to it.

Usage:  cluster_incompat.py <incompat.ndjson> [top-N]
"""

import json
import re
import sys
from collections import Counter, defaultdict

# Replace the parts of a message that vary per case with placeholders,
# so the remaining skeleton is the signature. Order matters: quoted
# spans first (they swallow identifiers), then bare identifiers/numbers.
SUBS = [
    (re.compile(r"`[^`]*`"), "`X`"),
    (re.compile(r"'[^']*'"), "'X'"),
    (re.compile(r'"[^"]*"'), '"X"'),
    (re.compile(r"\bline \d+\b"), "line N"),
    (re.compile(r"\bcol(?:umn)? \d+\b"), "col N"),
    (re.compile(r"\bat \d+\b"), "at N"),
    (re.compile(r"\b\d+\b"), "N"),
]

# Surfaces that belong to a documented post-v1.0 phase (P14 Proxy/Reflect,
# P16 multi-thread, and the Temporal / TypedArray / Intl backlog items).
# They are excluded from the gate predicate — not from the census, where
# they are reported separately so the split stays visible.
POST_V1_PATH = ("Temporal", "TypedArray", "Atomics", "SharedArrayBuffer", "Proxy", "Reflect")
POST_V1_MSG = ("Temporal", "Intl")


def is_post_v1(path: str, msg: str) -> bool:
    if path.startswith("test/intl402"):
        return True
    if any(t in path for t in POST_V1_PATH):
        return True
    return any(t in msg for t in POST_V1_MSG)


def signature(msg: str) -> str:
    """Collapse a message to its cluster key.

    `unknown identifier` keeps the actual name — that name IS the
    signal (`__this` and `eval` are different problems). `__new_*` folds
    into one key because the varying half is the user's class name, not
    a distinct gap.
    """
    m = re.search(r"unknown identifier `([^`]*)`", msg)
    if m:
        name = m.group(1)
        if name.startswith("__new_"):
            return "unknown identifier `__new_*` (new-on-a-function desugar)"
        return f"unknown identifier `{name}`"
    s = msg.strip()
    for pat, rep in SUBS:
        s = pat.sub(rep, s)
    return re.sub(r"\s+", " ", s)[:160]


def feature_dir(path: str, depth: int = 3) -> str:
    return "/".join(path.split("/")[:depth])


def main() -> None:
    src = sys.argv[1] if len(sys.argv) > 1 else "incompat.ndjson"
    top = int(sys.argv[2]) if len(sys.argv) > 2 else 40

    rows = []
    with open(src) as f:
        for line in f:
            line = line.strip()
            if line:
                rows.append(json.loads(line))

    total = len(rows)
    core = [r for r in rows if not is_post_v1(r["path"], r["msg"])]
    n_core = len(core)
    print(f"incompatible: {total}   core: {n_core}   post-v1.0 surface: {total - n_core}\n")

    print("=== layer 1 — kind (what stage rejected it) ===")
    for k, n in Counter(r["kind"] for r in rows).most_common():
        print(f"{n:7}  {n * 100 / total:5.1f}%  {k}")

    clusters = defaultdict(list)
    for r in core:
        clusters[signature(r["msg"])].append(r["path"])
    ranked = sorted(clusters.items(), key=lambda kv: -len(kv[1]))

    print(f"\n=== layer 2 — top {top} core clusters (of {len(ranked)}) ===")
    cum = 0
    for i, (sig, paths) in enumerate(ranked[:top], 1):
        n = len(paths)
        cum += n
        dirs = Counter(feature_dir(p) for p in paths)
        head = ", ".join(f"{d}({c})" for d, c in dirs.most_common(3))
        print(f"\n{i:3}. {n:6} cases  {n * 100 / n_core:4.1f}%  cum {cum * 100 / n_core:4.1f}%  dirs={len(dirs)}")
        print(f"     sig:  {sig}")
        print(f"     {head}")
        print(f"     e.g.  {paths[0]}")

    print("\n=== layer 3 — core mass by feature dir ===")
    for d, n in Counter(feature_dir(r["path"]) for r in core).most_common(20):
        print(f"{n:7}  {n * 100 / n_core:5.1f}%  {d}")

    print("\n=== coverage curve ===")
    for cut in (10, 25, 50, 100, 200, 400):
        s = sum(len(p) for _, p in ranked[:cut])
        print(f"  top{cut:4} clusters -> {s:6} / {n_core} = {s * 100 / n_core:5.1f}%")

    # The gate predicate. Keep this block last and keep its shape stable:
    # the rotation-close report quotes these four numbers verbatim, and
    # a moved line is a broken report.
    big = [(s, p) for s, p in ranked if len(p) >= 4]
    small = [(s, p) for s, p in ranked if len(p) <= 3]
    big_cases = sum(len(p) for _, p in big)
    small_cases = sum(len(p) for _, p in small)
    print("\n=== gate predicate (roadmap P-SURF S7.2) ===")
    print(f"  clusters >= 4 cases : {len(big)}      <- drives to 0 for v1.0")
    print(f"  cases in them       : {big_cases}")
    print(f"  clusters <= 3 cases : {len(small)} ({small_cases} cases, {small_cases * 100 / n_core:.1f}% — acceptable residue)")
    print(f"  core total          : {n_core}")


if __name__ == "__main__":
    main()
