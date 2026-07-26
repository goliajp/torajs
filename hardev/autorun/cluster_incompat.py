#!/usr/bin/env python3
"""Cluster the test262 `incompatible` bucket by root-cause signature.

Input: the ndjson produced by `torajs-test262 --incompat-ndjson`, one
`{"path","kind","msg"}` object per line.

The question this answers: of the ~39k cases the checker rejects, how
much mass sits behind a small number of substrate gaps? The stdout
breakdown only has six `incompat_kind` prefixes, which is too coarse to
tell "one missing feature blocking 4000 cases" from "4000 distinct
one-off rejects".

Two axes per cluster:
  - size            — how many cases share the signature
  - directory spread — how many distinct test262 feature dirs it spans.
                       A big cluster confined to one dir is a feature
                       gap; a big cluster spread over 50 dirs is a
                       cross-cutting substrate gap (worth far more).
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
    (re.compile(r"\b\d+\b"), "N"),
]


def signature(msg: str) -> str:
    s = msg.strip()
    for pat, rep in SUBS:
        s = pat.sub(rep, s)
    # Collapse whitespace and cap length — long tails of a message rarely
    # add discriminating power and would fragment otherwise-equal clusters.
    s = re.sub(r"\s+", " ", s)
    return s[:160]


def feature_dir(path: str, depth: int = 3) -> str:
    parts = path.split("/")
    return "/".join(parts[:depth])


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
    print(f"incompatible cases: {total}\n")

    kinds = Counter(r["kind"] for r in rows)
    print("=== layer 1 — kind (what stage rejected it) ===")
    for k, n in kinds.most_common():
        print(f"{n:7}  {n * 100 / total:5.1f}%  {k}")

    clusters = defaultdict(list)
    for r in rows:
        clusters[(r["kind"], signature(r["msg"]))].append(r["path"])

    print(f"\n=== layer 2 — top {top} message signatures (of {len(clusters)}) ===")
    cum = 0
    ranked = sorted(clusters.items(), key=lambda kv: -len(kv[1]))
    for i, ((kind, sig), paths) in enumerate(ranked[:top], 1):
        n = len(paths)
        cum += n
        dirs = Counter(feature_dir(p) for p in paths)
        head = ", ".join(f"{d}({c})" for d, c in dirs.most_common(3))
        print(f"\n{i:3}. {n:6} cases  {n * 100 / total:4.1f}%  cum {cum * 100 / total:4.1f}%  [{kind}]")
        print(f"     sig:  {sig}")
        print(f"     dirs: {len(dirs)} distinct — {head}")
        print(f"     e.g.  {paths[0]}")

    tail = total - cum
    print(f"\ntop {top} clusters cover {cum} / {total} = {cum * 100 / total:.1f}%")
    print(f"remaining {tail} cases spread over {len(clusters) - top} signatures")

    print("\n=== layer 3 — mass by feature dir (any kind) ===")
    bydir = Counter(feature_dir(r["path"]) for r in rows)
    for d, n in bydir.most_common(25):
        print(f"{n:7}  {n * 100 / total:5.1f}%  {d}")


if __name__ == "__main__":
    main()
