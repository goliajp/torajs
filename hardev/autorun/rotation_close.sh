#!/usr/bin/env bash
#
# hardev autorun pillar — run the four mechanical checks a rotation closes on.
#
# Steps 0b / 0c / 0c-2 of the close sequence in
# `rules/torajs-autorun-pipeline.md`, in the one order that is correct,
# with the HEAD sha passed through to every stamp. Each of those details
# has been got wrong before, and each failure looked like data:
#
#   - the sweep runs `target/release/tr`, NOT the `target/iter/tr` the
#     conformance gate builds, so it needs release-build.sh first or it
#     measures a stale substrate (rotation 226).
#   - release-build.sh exits 0 even when cargo inside it failed, so its
#     log has to be grepped for `^error` (rotation 408).
#   - mini takes source by rsync while its git HEAD stays behind, so a
#     locally-derived sha names the wrong commit. Every stamp gets the
#     dev-side sha explicitly (rotation 598).
#   - a sweep leaves `torajs-run-new-*` orphans burning cores until
#     something kills them (rotation 139).
#
# Run FROM the dev machine:
#   scp hardev/autorun/rotation_close.sh mini:/tmp/
#   ssh mini 'zsh -lc "bash /tmp/rotation_close.sh <dev-HEAD>"'
#   scp mini:'~/workspace/goliajp/torajs/hardev/{determinism,gmalloc}-latest.json' hardev/
#   scp mini:/tmp/t262-<HEAD>.json hardev/test262-latest.json
#
# The stamps must then be committed: TRIG-6 reads them out of the repo,
# not off mini.
#
# exit: 0 all four green · 1 at least one red (the summary says which)

set -u
[ "$#" -ge 1 ] || { echo "usage: rotation_close.sh <head-sha>" >&2; exit 2; }
H=$1
cd "$(dirname "$0")/../.." || exit 2
rc=0
note() { printf '\n=== %s ===\n' "$1"; }

note "release-build (the sweep runs target/release/tr, not the gate's iter tr)"
bash scripts/release-build.sh > /tmp/rc-rb.log 2>&1
errs=$(grep -cE '^error' /tmp/rc-rb.log)
echo "release_build_errors=$errs"
[ "$errs" -eq 0 ] || { echo "release-build FAILED — see /tmp/rc-rb.log" >&2; exit 1; }
cargo build --release -p torajs-test262 > /tmp/rc-rbt.log 2>&1 || {
  echo "torajs-test262 build FAILED — see /tmp/rc-rbt.log" >&2; exit 1; }

note "0b — test262 sweep"
./target/release/torajs-test262 --workers 10 --head-sha "$H" \
  --json "/tmp/t262-$H.json" --dump-verdicts "/tmp/t262-$H-verdicts.txt" \
  --incompat-ndjson "/tmp/t262-$H-incompat.ndjson" \
  --bugs-ndjson "/tmp/t262-$H-bugs.ndjson" > /tmp/rc-sweep.log 2>&1
sweep_rc=$?
tail -3 /tmp/rc-sweep.log
[ "$sweep_rc" -eq 0 ] || rc=1

# Orphans first: a hung fixture's spawn-child reparents to init and keeps
# a core busy indefinitely. Cheap here, expensive to notice hours later.
pkill -f torajs-run-new 2>/dev/null
rm -f /var/folders/*/T/torajs-run-new-* /tmp/torajs-run-new-* 2>/dev/null

note "0c — build determinism"
HARDEV_HEAD_SHA="$H" bash hardev/autorun/build_determinism.sh > /tmp/rc-bd.log 2>&1
bd_rc=$?
tail -1 /tmp/rc-bd.log
[ "$bd_rc" -eq 0 ] || rc=1

note "0c-2 — Guard Malloc"
HARDEV_HEAD_SHA="$H" bash hardev/autorun/gmalloc_scan.sh > /tmp/rc-gm.log 2>&1
gm_rc=$?
tail -2 /tmp/rc-gm.log
[ "$gm_rc" -eq 0 ] || rc=1

note "summary"
printf 'sweep=%s determinism=%s gmalloc=%s  (stamp json: /tmp/t262-%s.json, hardev/{determinism,gmalloc}-latest.json)\n' \
  "$sweep_rc" "$bd_rc" "$gm_rc" "$H"
exit "$rc"
