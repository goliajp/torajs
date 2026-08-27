#!/bin/bash
# Build a sample of conformance fixtures with the RELEASE `tr build`
# and run each artifact against bun. This is the gate the conformance
# gate cannot be: `tr run` links the iter staticlibs (with std, so
# nothing is ever empty and every writer entry is live), while the
# shipped artifact links the release archives through dead-strip,
# link-judged stubs and elided sites. Two whole classes of defect
# exist only there:
#
#   - a link shape that only a small program takes (r505: a __DATA
#     segment with no file bytes produced a zero-length chained-fixups
#     payload and dyld refused the binary);
#   - a link judgment that stubs a slow path the program reaches
#     (r506: two writer entries merged onto one atom, the guard read
#     one name, and every static-`this` class program died with
#     `closure props drop stripped (link judgment bug)`).
#
# Same family as build_determinism.sh / gmalloc_scan.sh: a defect
# class every other gate is green on, bought mechanically.
#
# RUN AFTER `scripts/release-build.sh`. The gate rebuilds
# target/release/tr as the HOST profile (r503); a stale or host tr
# here measures nothing. Never run while a gate is running.
#
# usage: hardev/autorun/release_batch.sh [stride] [extra-prefix-regex]
#   stride: take every Nth fixture (default 41 -> ~75 of 3000);
#   extra-prefix-regex: additionally take every 3rd fixture whose
#     name matches (default: the class / closure families, where the
#     link judgments live).
# exit 0 = every sampled artifact built and matched bun.

set -u

STRIDE="${1:-41}"
EXTRA="${2:-^(class|inherit|super|abstract|vtable|closure|fn-|fnexpr)}"

cd "$(dirname "$0")/../.." || exit 2
TR=target/release/tr
[ -x "$TR" ] || { echo "release_batch: $TR missing — run scripts/release-build.sh first" >&2; exit 2; }
command -v bun >/dev/null || { echo "release_batch: bun not on PATH (use zsh -lc)" >&2; exit 2; }

TMP="$(mktemp -d "${TMPDIR:-/tmp}/hardev-rb.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

n=0; neq=0; buildfail=0
for f in $(ls conformance/cases | awk -v s="$STRIDE" 'NR % s == 1') \
         $(ls conformance/cases | grep -E "$EXTRA" | awk 'NR % 3 == 0'); do
  case "$f" in *.ts) ;; *) continue ;; esac
  n=$((n + 1))
  if ! "$TR" build "conformance/cases/$f" -o "$TMP/a.bin" >"$TMP/build.log" 2>&1; then
    buildfail=$((buildfail + 1)); echo "BUILDFAIL $f: $(tail -1 "$TMP/build.log")"; continue
  fi
  a=$("$TMP/a.bin" 2>&1); b=$(bun "conformance/cases/$f" 2>&1)
  if [ "$a" != "$b" ]; then neq=$((neq + 1)); echo "NEQ $f"; fi
done
echo "release_batch: n=$n neq=$neq buildfail=$buildfail"
[ "$neq" -eq 0 ] && [ "$buildfail" -eq 0 ]
