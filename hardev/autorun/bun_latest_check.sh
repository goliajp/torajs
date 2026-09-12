#!/usr/bin/env bash
#
# hardev autorun pillar — is the bun we benchmark against the latest one?
#
# Why this exists. The design principle says the comparators must be
# latest-stable and the results file must carry the full version; until
# 2026-09-12 the only enforcement was the sentence itself. bun ships a
# patch release every few days (1.4.0 08-20, 1.4.1 09-04, 1.4.2 09-05),
# and a comparator that quietly falls a release behind turns every
# tr/bun ratio into a comparison against a competitor that no longer
# exists. takagi 2026-09-12: 「一定要保持更新」.
#
# It reads four things and compares each against the upstream tag:
#
#   upstream   gh release view oven-sh/bun          (the truth)
#   dev        bun --version on this machine
#   mini       bun --version on mini, via zsh -lc   (PATH needs the login shell)
#   stamped    "bunStamped" in hardev/bench-latest.json
#
# `bun --version` is the only trustworthy host reading: `bun upgrade`
# overwrites the brew Cellar binary in place and the directory name never
# changes, so `which` / mtime / the path all lie.
#
# usage:
#   bun_latest_check.sh              # all four readings
#   HARDEV_BUN_HOSTS=none …          # upstream vs stamp only (the gate)
#   HARDEV_BUN_HOSTS=dev …           # skip the ssh to mini
#   HARDEV_BUN_UPSTREAM=1.4.2 …      # pin upstream (self-test, offline)
#
# exit: 0 every reading equals upstream · 1 some reading is behind ·
#       2 a reading came back empty (the instrument failed — that is not
#       "current", it is "unknown", and the two must never look alike)

set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
STAMP="${HARDEV_BENCH_JSON:-$PROJECT_DIR/hardev/bench-latest.json}"
HOSTS="${HARDEV_BUN_HOSTS:-dev mini}"

upstream="${HARDEV_BUN_UPSTREAM:-}"
if [ -z "$upstream" ]; then
  upstream=$(gh release view --repo oven-sh/bun --json tagName -q .tagName 2>/dev/null)
  upstream=${upstream#bun-v}
fi
if [ -z "$upstream" ]; then
  echo "bun: upstream=? status=unknown (gh release view oven-sh/bun returned nothing)"
  exit 2
fi

declare -a behind=() unknown=()
line="bun: upstream=$upstream"

read_host() {
  case "$1" in
    dev)  bun --version 2>/dev/null ;;
    mini) ssh -o BatchMode=yes mini 'zsh -lc "bun --version"' 2>/dev/null ;;
  esac
}

for h in $HOSTS; do
  [ "$h" = none ] && continue
  v=$(read_host "$h" | tr -d '[:space:]')
  if [ -z "$v" ]; then
    line="$line $h=?"; unknown+=("$h")
  else
    line="$line $h=$v"
    [ "$v" = "$upstream" ] || behind+=("$h=$v")
  fi
done

if [ -f "$STAMP" ]; then
  s=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1])).get("bunStamped") or "")' "$STAMP" 2>/dev/null)
  if [ -z "$s" ]; then
    line="$line stamped=?"; unknown+=("stamped")
  else
    line="$line stamped=$s"
    [ "$s" = "$upstream" ] || behind+=("stamped=$s")
  fi
else
  line="$line stamped=?"; unknown+=("stamped(no $STAMP)")
fi

if [ "${#unknown[@]}" -gt 0 ]; then
  echo "$line status=unknown (${unknown[*]})"
  exit 2
fi
if [ "${#behind[@]}" -gt 0 ]; then
  echo "$line status=behind (${behind[*]}) — bun upgrade on the host, rerun bench, then bench_latest.sh"
  exit 1
fi
echo "$line status=current"
exit 0
