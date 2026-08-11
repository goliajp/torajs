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
from pathlib import Path

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
#
# ArrayBuffer / DataView ride with the TypedArray backlog item: they are
# its substrate (a DataView without typed arrays serves nothing), and
# counting them as core would put ~950 cases on the v1.0 gate that the
# TypedArray phase will absorb anyway.
POST_V1_PATH = (
    "Temporal",
    "TypedArray",
    "Atomics",
    "ArrayBuffer",  # covers SharedArrayBuffer too
    "DataView",
    "Proxy",
    "Reflect",
)
POST_V1_MSG = ("Temporal", "Intl")

# A case whose unknown identifier IS a post-v1.0 global is blocked on
# that surface no matter which directory it lives in (`new Proxy(...)`
# as a test fixture under Object/keys is still a Proxy case). Matched
# against the extracted name exactly — never as a message substring,
# which would misfire on core cases that merely mention these words.
POST_V1_GLOBALS = frozenset(
    ["Temporal", "Intl", "Proxy", "Reflect", "Atomics", "ShadowRealm"]
    + ["ArrayBuffer", "SharedArrayBuffer", "DataView"]
    + [f"{p}{w}Array" for p in ("Ui", "I") for w in ("nt8", "nt16", "nt32")]
    + ["Uint8ClampedArray", "Float16Array", "Float32Array", "Float64Array"]
    + ["BigInt64Array", "BigUint64Array"]
)

UNKNOWN_IDENT = re.compile(r"unknown ident(?:ifier)? `([^`]*)`")


def is_post_v1(path: str, msg: str) -> bool:
    if path.startswith("test/intl402"):
        return True
    if any(t in path for t in POST_V1_PATH):
        return True
    if any(t in msg for t in POST_V1_MSG):
        return True
    m = UNKNOWN_IDENT.search(msg)
    if m:
        name = m.group(1).removeprefix("__new_")
        return name in POST_V1_GLOBALS
    return False


def signature(msg: str) -> str:
    """Collapse a message to its cluster key.

    The two unknown-name shapes (checker `unknown identifier`, ssa-lower
    `unknown ident`) keep the actual name — that name IS the signal
    (`__this` and `eval` are different problems, and so are `callbackfn`
    and `Math`). `__new_*` folds into one key because the varying half
    is the user's class name, not a distinct gap.
    """
    m = UNKNOWN_IDENT.search(msg)
    if m:
        name = m.group(1)
        stage = "ssa-lower " if "ssa-lower" in msg else ""
        if name.startswith("__new_"):
            return f"{stage}unknown ident `__new_*` (new-on-a-function desugar)"
        return f"{stage}unknown ident `{name}`"
    s = msg.strip()
    for pat, rep in SUBS:
        s = pat.sub(rep, s)
    return re.sub(r"\s+", " ", s)[:160]


def feature_dir(path: str, depth: int = 3) -> str:
    return "/".join(path.split("/")[:depth])


FLAGS_RE = re.compile(r"flags:\s*\[([^\]]*)\]")
FRONTMATTER_RE = re.compile(r"/\*---(.*?)---\*/", re.S)


def load_register():
    """Load the subset-decision register (roadmap S7.2) next to this
    script. Missing file = empty register, never an error — the census
    must stay runnable on a bare checkout."""
    reg_path = Path(__file__).with_name("subset_register.json")
    if not reg_path.exists():
        return []
    return json.loads(reg_path.read_text())["entries"]


def attributed_paths(entries, paths):
    """Map register entries to the subset of `paths` each predicate
    attributes. Predicates are mechanical — anything that needs
    eyeballing does not belong in the register. The test262 corpus is
    resolved relative to the repo root (two levels up from this
    script); if it is absent the predicate CANNOT run, and the honest
    answer is to attribute nothing and say so, not to guess."""
    corpus = Path(__file__).resolve().parents[2] / "vendor" / "test262"
    out = {}
    for e in entries:
        pred = e["predicate"]
        if pred["kind"] == "test262-flag":
            if not corpus.is_dir():
                print(f"  !! register {e['id']}: corpus not found at {corpus}, predicate skipped")
                out[e["id"]] = set()
                continue
            hit = set()
            for p in paths:
                try:
                    src = (corpus / p).read_text(encoding="utf-8", errors="replace")
                except OSError:
                    continue
                m = FRONTMATTER_RE.search(src)
                if not m:
                    continue
                fl = FLAGS_RE.search(m.group(1))
                if fl and pred["flag"] in fl.group(1):
                    hit.add(p)
            out[e["id"]] = hit
        else:
            print(f"  !! register {e['id']}: unknown predicate kind {pred['kind']!r}, skipped")
            out[e["id"]] = set()
    return out


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
    # a moved line is a broken report. Since 2026-08-11 the four numbers
    # are register-aware: the predicate counts *unattributed* clusters
    # (roadmap S7.2 — "resolved, or attributed"), so register-attributed
    # cases are removed before clustering and reported beside the gate.
    print("\n=== gate predicate (roadmap P-SURF S7.2) ===")
    entries = load_register()
    attr = attributed_paths(entries, [r["path"] for r in core])
    attr_all = set().union(*attr.values()) if attr else set()
    for e in entries:
        print(f"  register {e['id']} ({e['title']}): {len(attr[e['id']])} cases attributed")
    if entries:
        print(f"  register total      : {len(entries)} entries, {len(attr_all)} of {n_core} core cases attributed")

    un_clusters = defaultdict(list)
    for r in core:
        if r["path"] not in attr_all:
            un_clusters[signature(r["msg"])].append(r["path"])
    un_ranked = sorted(un_clusters.items(), key=lambda kv: -len(kv[1]))
    big = [(s, p) for s, p in un_ranked if len(p) >= 4]
    small = [(s, p) for s, p in un_ranked if len(p) <= 3]
    big_cases = sum(len(p) for _, p in big)
    small_cases = sum(len(p) for _, p in small)
    n_un = n_core - len(attr_all)
    print(f"  clusters >= 4 cases : {len(big)}      <- drives to 0 for v1.0 (unattributed)")
    print(f"  cases in them       : {big_cases}")
    print(f"  clusters <= 3 cases : {len(small)} ({small_cases} cases, {small_cases * 100 / n_core:.1f}% — acceptable residue)")
    print(f"  core total          : {n_core} ({n_un} unattributed)")


if __name__ == "__main__":
    main()
