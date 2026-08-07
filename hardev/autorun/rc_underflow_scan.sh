#!/bin/zsh
# Census of refcount underflows across the conformance corpus.
#
# An underflow means the codegen handed two owners one reference: the
# second release runs `0 - 1` on freed memory, wraps the count to
# u32::MAX, and the block is never freed nor scrubbed from the
# cycle-root buffer. The program usually keeps running — the crash, if
# it comes at all, lands somewhere unrelated (rotation 323 met one as a
# segfault inside `__torajs_cycle_at_exit_drain`, three lowering layers
# from the defect).
#
# That is why this script exists: 41 of the 2540 conformance cases were
# corrupting memory while the gate, nextest and a full test262 sweep all
# reported green. Byte-equal stdout does not imply a balanced heap, so
# no existing gate can see this class.
#
# Usage:  zsh hardev/autorun/rc_underflow_scan.sh [baseline]
#   baseline defaults to RC_UNDERFLOW_BASELINE below. Exit 0 when the
#   count is at or under it, 1 when it rises (a new imbalance shipped),
#   2 on build failure. Lowering the baseline after fixes is the point;
#   raising it is not a fix.
#
# Cost: ~5 min the first time (cold build into an isolated target dir),
# ~1-2 min after (the dir is kept warm), plus ~1 min of scanning.

set -u
cd "$(dirname "$0")/../.." || exit 2

# Cases known to underflow. Started at 41 when the detector landed
# (rotation 323, HEAD 82e30c55); every one of those predated it.
# 41 → 32: the iterator `entries()` pre-dec-to-0 idiom, one defect
# across torajs-arr and torajs-collections.
# See `.claude/plan-state.md` for the remaining clusters.
RC_UNDERFLOW_BASELINE=32
baseline=${1:-$RC_UNDERFLOW_BASELINE}

# NEVER build into ./target: a detector-instrumented `tr` left in
# target/release would silently poison every later gate and sweep, and
# those read the binary without checking how it was built.
export CARGO_TARGET_DIR="${TMPDIR:-/tmp}/torajs-rc-underflow-target"
export RUSTFLAGS="--cfg torajs_rc_underflow_detect"

echo "building detector tr into $CARGO_TARGET_DIR (RUSTFLAGS=$RUSTFLAGS)"
# Whole workspace, and possibly more than once. torajs-core
# `include_bytes!`s every `libtorajs_*.a` out of this profile directory
# but has no cargo dependency edge on the crates that produce them, so
# nothing orders them ahead of it — a warm ./target only works because
# the files are already there from previous builds. On a cold dir the
# first pass fails inside torajs-core while still emitting the
# staticlibs it was missing, and the next pass finds them. Converges in
# two; three is slack.
rt=1
for attempt in 1 2 3; do
  caffeinate -i cargo build --workspace --release > /tmp/rc-uf-build-rt.log 2>&1
  rt=$?
  [ $rt -eq 0 ] && break
  echo "  workspace pass $attempt incomplete (cold staticlib dir), retrying"
done
if [ $rt -ne 0 ]; then
  echo "BUILD FAILED (workspace, rt=$rt) — see /tmp/rc-uf-build-rt.log"
  exit 2
fi

caffeinate -i cargo build --release -p torajs-cli > /tmp/rc-uf-build-cli.log 2>&1
if [ $? -ne 0 ]; then
  echo "BUILD FAILED (torajs-cli) — see /tmp/rc-uf-build-cli.log"
  exit 2
fi

tr_bin="$CARGO_TARGET_DIR/release/tr"
if ! strings "$tr_bin" | grep -q __TORAJS_RC_UNDERFLOW__; then
  echo "detector marker absent from $tr_bin — the cfg did not reach the"
  echo "staticlib, so a clean scan here would be meaningless"
  exit 2
fi

out=${2:-/tmp/rc-underflow-cases.txt}
: > "$out"
print -l conformance/cases/*.ts \
  | xargs -P 10 -I{} zsh -c \
      '"$0" run "$1" 2>&1 | grep -q __TORAJS_RC_UNDERFLOW__ && echo "$1"' \
      "$tr_bin" {} \
  | sort > "$out"

count=$(wc -l < "$out" | tr -d ' ')
echo "cases with refcount underflow: $count (baseline $baseline)"
echo "list: $out"
if [ "$count" -gt "$baseline" ]; then
  echo "REGRESSION — $((count - baseline)) new case(s) underflow"
  exit 1
fi
[ "$count" -lt "$baseline" ] && echo "improved by $((baseline - count)) — lower RC_UNDERFLOW_BASELINE"
exit 0
