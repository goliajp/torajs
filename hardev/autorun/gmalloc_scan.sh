#!/bin/bash
# Run every conformance fixture's AOT artifact under Apple's Guard
# Malloc and report the ones that only die there. Those are
# use-after-free / out-of-bounds reads: real memory defects that
# produce correct-looking output today because the freed bytes are
# still readable.
#
# WHY THIS EXISTS. The conformance gate compares stdout, so it is
# structurally blind to this whole class. Rotation 384's defect is the
# proof: its fixture matched bun byte for byte WITHOUT the fix, and 400
# allocations of churn afterwards still did not make it show. Guard
# Malloc unmaps freed pages, so the same read becomes a deterministic
# SIGSEGV -- which is what turned a heisenbug that had been misattributed
# to an unrelated commit into a five-line reproduction.
#
# Same family as build_determinism.sh: a defect class every other gate
# is green on, bought with a mechanical check rather than with vigilance.
#
# GUARD THE ARTIFACT, NEVER THE COMPILER. `tr run` compiles in-process,
# so guarding it puts the whole compiler under a page-per-allocation
# allocator. Ten of those concurrently took the build machine down with
# a kernel watchdog panic (rotation 384). Here we `tr build` unguarded
# and guard only the resulting program, which allocates a handful of
# cells. Keep the worker count low anyway -- Guard Malloc's cost is
# virtual address space, not CPU, and an allocation-heavy fixture can
# still balloon far enough for the kernel to jetsam it. Those land in
# the "verdict unknown" bucket rather than being called defects.
#
# usage: hardev/autorun/gmalloc_scan.sh [workers] [case-glob]
# exit 0 = no fixture died only under the guard.

set -u

WORKERS="${1:-2}"
GLOB="${2:-conformance/cases/*.ts}"
GUARD=/usr/lib/libgmalloc.dylib
TIMEOUT_SECS=60

cd "$(dirname "$0")/../.." || exit 2

if [ ! -f "$GUARD" ]; then
  echo "gmalloc_scan: $GUARD not present -- macOS only" >&2
  exit 2
fi

TR="$(cargo metadata --no-deps --format-version 1 |
  python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])')/release/tr"
if [ ! -x "$TR" ]; then
  echo "gmalloc_scan: $TR missing -- run scripts/release-build.sh first" >&2
  exit 2
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/hardev-gm.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

# Bounded run of an already-built artifact. NOT via timeout(1) or
# perl: macOS SIP strips DYLD_* when exec'ing a protected system
# binary, so wrapping the guarded run in one silently drops the guard
# and the scan reports everything clean. The first version of this
# script did exactly that, and only a self-test against a known-bad
# build caught it.
# Returns the child's exit status, or 200 if OUR watchdog killed it.
# Those must not be confused: the watchdog's `kill -9` surfaces as 137,
# which is >= 128 and reads exactly like a crash. The first full scan
# reported 12 offenders and 11 of them were this -- slow-under-guard
# fixtures, not defects.
run_bounded() {
  local guarded="$1" bin="$2" pid watchdog rc
  local flag="$WORK/timedout.$$"
  rm -f "$flag"
  if [ "$guarded" = yes ]; then
    DYLD_INSERT_LIBRARIES="$GUARD" "$bin" >/dev/null 2>&1 &
  else
    "$bin" >/dev/null 2>&1 &
  fi
  pid=$!
  { sleep "$TIMEOUT_SECS"; : >"$flag"; kill -9 "$pid" 2>/dev/null; } &
  watchdog=$!
  wait "$pid"
  rc=$?
  kill "$watchdog" 2>/dev/null
  wait "$watchdog" 2>/dev/null
  if [ -f "$flag" ]; then
    rm -f "$flag"
    return 200
  fi
  return "$rc"
}

probe_one() {
  local f="$1" bin plain guarded
  bin="$WORK/$(echo "$f" | tr '/.' '__')"
  if ! "$TR" build "$f" -o "$bin" >/dev/null 2>&1; then
    printf '%s\tBUILD-FAILED\n' "$f" >>"$WORK/skipped"
    return 0
  fi
  run_bounded no "$bin"
  plain=$?
  run_bounded yes "$bin"
  guarded=$?
  # 200 = our watchdog. 137 = SIGKILL, which a program cannot raise on
  # itself -- under the guard it means the kernel jetsammed it for
  # memory. Neither is evidence of a defect, and calling them one is
  # how the first full scan reported 12 offenders of which 11 were the
  # measuring apparatus.
  if [ "$guarded" = 200 ] || [ "$plain" = 200 ] ||
    [ "$guarded" = 137 ] || [ "$plain" = 137 ]; then
    printf '%s\tplain=%s\tguarded=%s\n' "$f" "$plain" "$guarded" >>"$WORK/timeouts"
    rm -f "$bin"
    return 0
  fi
  # Only a guarded death the plain run did not produce counts: a
  # fixture whose uncaught throw exits 1 both ways is fine, and one
  # that is slow both ways trips the plain side too.
  if [ "$guarded" -ge 128 ] && [ "$plain" -lt 128 ]; then
    printf '%s\tplain=%s\tguarded=%s\n' "$f" "$plain" "$guarded" >>"$WORK/offenders"
  fi
  rm -f "$bin"
}
export -f probe_one run_bounded
export TR GUARD WORK TIMEOUT_SECS

: >"$WORK/offenders"
: >"$WORK/skipped"
: >"$WORK/timeouts"

# shellcheck disable=SC2086
ls $GLOB >"$WORK/cases" 2>/dev/null
TOTAL=$(wc -l <"$WORK/cases" | tr -d ' ')
echo "gmalloc_scan: $TOTAL fixtures, $WORKERS workers, tr = $TR"

xargs -P "$WORKERS" -I{} bash -c 'probe_one "$@"' _ {} <"$WORK/cases"

SKIPPED=$(wc -l <"$WORK/skipped" | tr -d ' ')
TIMEDOUT=$(wc -l <"$WORK/timeouts" | tr -d ' ')
COUNT=$(wc -l <"$WORK/offenders" | tr -d ' ')
if [ "$SKIPPED" -gt 0 ]; then
  echo "gmalloc_scan: $SKIPPED fixture(s) would not AOT-build, not scanned:"
  sort "$WORK/skipped"
fi
if [ "$TIMEDOUT" -gt 0 ]; then
  echo "gmalloc_scan: $TIMEDOUT fixture(s) too heavy under the guard (timeout / jetsam), verdict unknown:"
  sort "$WORK/timeouts"
fi
if [ "$COUNT" -eq 0 ]; then
  echo "gmalloc_scan: $((TOTAL - SKIPPED - TIMEDOUT))/$TOTAL scanned clean"
  exit 0
fi
echo "gmalloc_scan: $COUNT fixture(s) die ONLY under Guard Malloc (use-after-free / OOB):"
sort "$WORK/offenders"
exit 1
