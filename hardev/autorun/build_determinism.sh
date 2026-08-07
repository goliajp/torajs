#!/usr/bin/env bash
#
# hardev autorun pillar — does every bench case still compile, and does
# each one produce exactly one artifact when built repeatedly?
#
# Why this exists:
#   Rotation 321 closed four independent hash-order leaks in the
#   compiler (guard grouping, phi numbering, split-point scan, plus a
#   `DfaState` padding byte read uninitialised into `__DATA_CONST`).
#   The closing measurement — 44/44 cases each producing a single
#   artifact sha at N=12 — was a one-shot run typed by hand. Nothing
#   stopped the next `HashMap`/`HashSet` iteration from re-introducing
#   the same class of bug, and the symptom is invisible in every gate
#   we run: conformance passes, tests pass, only the bytes differ.
#   This script is that missing gate, and it is why the check belongs
#   beside `cargo fmt --check` and the 0-warning build rather than in a
#   rotation's prose.
#
#   It also answers the cheaper question in the same pass: does the
#   case compile at all? Rotation 320 shipped with one bench case
#   silently failing to build, found only because someone looked.
#
# Usage:
#   build_determinism.sh [N]        # N = builds per case, default 12
#
# Behaviour:
#   - Builds every `bench/cases/*/main.ts` once to check compilation,
#     then N more times, hashing the artifact after each build.
#   - Emits CSV on stdout: `case,build_ok,distinct_sha`.
#   - Emits a summary line carrying N. **Never report a determinism
#     count without the N that produced it** — rotation 320 sampled at
#     N=5 and missed four nondeterministic cases; N=8 still shows
#     `json-stringify` and `regex-dfa-iflag` as falsely deterministic.
#     N>=10 is the floor; the default of 12 leaves margin.
#   - Resolves `tr` through `cargo metadata`, never through a relative
#     `./target` path (see torajs-autorun-pipeline.md — the per-project
#     `target` symlink is an anti-pattern and drifts).
#
# Env vars:
#   HARDEV_TR_PROFILE   cargo profile holding the tr to test
#                       (default: iter — the profile the conformance
#                       gate itself runs; use `release` to check the
#                       AOT-facing binary built by release-build.sh)
#
# Exit codes:
#   0 — every case compiled and every case produced exactly one sha
#   1 — at least one case failed to build, or is nondeterministic
#   2 — usage error
#   3 — could not locate a `tr` binary to test

set -u

if [ $# -gt 1 ]; then
  echo "usage: build_determinism.sh [N]" >&2
  exit 2
fi

N="${1:-12}"
case "$N" in
  ''|*[!0-9]*) echo "build_determinism.sh: N must be a positive integer" >&2; exit 2 ;;
esac
if [ "$N" -lt 10 ]; then
  # Not a hard refusal — a fast smoke at N=3 is a legitimate thing to
  # want — but the caller must be told the reading cannot be compared
  # against the rotation baselines.
  echo "build_determinism.sh: warning — N=$N is below the N>=10 floor;" \
       "deterministic results at this N are not evidence" >&2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
PROFILE="${HARDEV_TR_PROFILE:-iter}"

TARGET_DIR=$(cd "$PROJECT_DIR" && cargo metadata --no-deps --format-version 1 2>/dev/null \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)
if [ -z "${TARGET_DIR:-}" ]; then
  echo "build_determinism.sh: cargo metadata gave no target_directory" >&2
  exit 3
fi
TR="$TARGET_DIR/$PROFILE/tr"
if [ ! -x "$TR" ]; then
  echo "build_determinism.sh: no tr at $TR (profile=$PROFILE)" >&2
  exit 3
fi

WORK=$(mktemp -d "${TMPDIR:-/tmp}/hardev-bd.XXXXXX") || exit 3
trap 'rm -rf "$WORK"' EXIT INT TERM

FAILED=0
NONDET=0
CASES=0

echo "case,build_ok,distinct_sha"
for dir in "$PROJECT_DIR"/bench/cases/*/; do
  src="$dir/main.ts"
  [ -f "$src" ] || continue
  name=$(basename "$dir")
  CASES=$((CASES + 1))

  if ! "$TR" build "$src" -o "$WORK/probe.bin" > "$WORK/$name.log" 2>&1; then
    echo "$name,FAIL,-"
    FAILED=$((FAILED + 1))
    continue
  fi

  i=0
  while [ "$i" -lt "$N" ]; do
    "$TR" build "$src" -o "$WORK/run.bin" > /dev/null 2>&1
    shasum -a 256 "$WORK/run.bin" | cut -c1-12
    i=$((i + 1))
  done | sort -u > "$WORK/shas.txt"

  distinct=$(wc -l < "$WORK/shas.txt" | tr -d ' ')
  echo "$name,ok,$distinct"
  [ "$distinct" -gt 1 ] && NONDET=$((NONDET + 1))
done

echo "--- cases=$CASES build_failures=$FAILED nondeterministic=$NONDET N=$N profile=$PROFILE"

if [ "$FAILED" -gt 0 ] || [ "$NONDET" -gt 0 ]; then
  exit 1
fi
exit 0
