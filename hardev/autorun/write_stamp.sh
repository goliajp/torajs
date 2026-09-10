#!/usr/bin/env bash
#
# hardev autorun pillar — write a run stamp for one mechanical check.
#
# Why stamps exist. Every gate this project runs reads what was *written*,
# not what was *done*. TRIG-3 checks `gate: N/F/S` for F=0 and never asks
# whether N is a real number; the sweep / determinism / Guard-Malloc lines
# in a handoff are free text. So the whole class of "it was not run", "it
# ran on the previous HEAD", "last rotation's result got carried forward"
# is invisible — and rules/torajs-autorun-pipeline.md records that class
# happening at least five times (sweep dark for five rotations while six
# pass regressions accumulated; a stale mini gz read as this rotation's
# baseline; release_batch neq compared across incomparable samples).
#
# A stamp is the same shape as `hardev/test262-latest.json`, which already
# works this way: a small JSON file, committed, carrying the sha of the
# source it measured. TRIG-6 then reads four facts instead of four
# sentences.
#
# What it does NOT do: stop someone from writing a false stamp. It closes
# the accidental failures — the ones that look exactly like success —
# not deliberate fabrication, which stays where it was, under
# honesty/no-fabricated-data.
#
# HEAD sha. On mini the working tree arrives by rsync while its git HEAD
# stays behind (see torajs-autorun-pipeline.md, "mini 的同步通道是 rsync"),
# so a locally-derived sha there names the wrong source. Callers running
# remotely MUST pass the real one via HARDEV_HEAD_SHA — the same
# convention `torajs-test262 --head-sha` already uses. When it is absent
# the stamp records `headShaSource: "local-git"` and marks a dirty tree,
# so a reader can tell a trustworthy sha from a guessed one.
#
# usage:
#   write_stamp.sh <out.json> <tool> <verdict> [key=value ...]
#     verdict : ok | fail
#     values  : plain strings, or int:N for a number
#
# exit: 0 on write, 2 if the output path is unwritable.

set -u

[ "$#" -ge 3 ] || { echo "write_stamp: usage: <out.json> <tool> <verdict> [k=v ...]" >&2; exit 2; }

OUT=$1; TOOL=$2; VERDICT=$3; shift 3

if [ -n "${HARDEV_HEAD_SHA:-}" ]; then
  SHA="$HARDEV_HEAD_SHA"; SRC=arg
else
  SHA="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"; SRC=local-git
  # The stamps themselves are excluded from the dirty test. Writing a
  # stamp makes the tree dirty, so including them means a stamp can never
  # be written clean: the first run marks itself `-dirty`, and committing
  # it only sets up the next run to do the same. What the flag is for is
  # "was the SOURCE this measured a committed state", and a stamp file is
  # not source.
  if [ -n "$(git status --porcelain 2>/dev/null | grep -vE 'hardev/[a-z0-9-]*-latest\.(json|txt\.gz)')" ]; then
    SHA="$SHA-dirty"
  fi
fi

mkdir -p "$(dirname "$OUT")" 2>/dev/null || { echo "write_stamp: cannot create $(dirname "$OUT")" >&2; exit 2; }

python3 - "$OUT" "$TOOL" "$VERDICT" "$SHA" "$SRC" "$@" <<'PY' || exit 2
import json, sys, datetime

out, tool, verdict, sha, src = sys.argv[1:6]
doc = {
    "tool": tool,
    "ranAt": datetime.datetime.now(datetime.timezone.utc)
                 .strftime("%Y-%m-%dT%H:%M:%SZ"),
    "headSha": sha,
    "headShaSource": src,
    "verdict": verdict,
}
for arg in sys.argv[6:]:
    if "=" not in arg:
        continue
    k, v = arg.split("=", 1)
    if v.startswith("int:"):
        try:
            doc[k] = int(v[4:])
        except ValueError:
            doc[k] = None
    else:
        doc[k] = v
with open(out, "w") as f:
    json.dump(doc, f, indent=2, sort_keys=False)
    f.write("\n")
print(f"stamp: {out} {tool} {verdict} headSha={sha} ({src})")
PY
