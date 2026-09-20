#!/usr/bin/env bash
#
# hardev autorun pillar — every tracked source file must be greppable.
#
# A single NUL byte anywhere in a file makes BSD `grep -r` classify it as
# binary and skip it **silently** — no "Binary file ... matches" line, no
# exit-code change, nothing. The result is indistinguishable from "this
# string does not appear in the repo", which is the most expensive shape a
# measuring device can fail in (perf methodology: "空输出/零值被当成测到了 0").
#
# Rotation 678 paid for this one: `check_type_of_call/route_globals.rs` — the
# checker cascade's own route file — carried a raw NUL inside a comment that
# was describing the string `"\0"`. Every `grep -r` in the repo had been
# skipping it, and four checker arms wired from that very file were read as
# dead code before an `ssh mini grep` (different locale, different grep)
# printed the line the local one had swallowed.
#
# The check is not "no NUL in .rs": it is "no NUL in anything git tracks",
# because the failure is a property of grep, not of Rust.
#
# Exit 0 = every tracked text file is greppable. Exit 1 = at least one is not.
set -u
cd "${1:-$(git rev-parse --show-toplevel)}" || exit 2

# The predicate measures the CAUSE (a NUL byte in the file), not grep's
# reaction to it. The first version of this script asked grep instead --
# "matches with -a but not without" -- and its positive control came back
# GREEN on a file that provably held a NUL: `grep -q` stops at the first
# match, and whether it has read far enough to notice the NUL by then
# depends on where the NUL sits. A guard whose own control passes for the
# wrong reason is the thing this pillar exists to prevent, so the question
# is now counted in bytes.
bad=0
while IFS= read -r f; do
  [ -f "$f" ] || continue
  n=$(LC_ALL=C tr -dc '\000' < "$f" | wc -c | tr -d ' ')
  if [ "$n" -gt 0 ]; then
    echo "NOT-GREPPABLE  $f  (NUL bytes: $n)"
    bad=$((bad + 1))
  fi
done < <(git ls-files -- '*.rs' '*.ts' '*.md' '*.toml' '*.sh' '*.py' '*.json')

echo "--- greppable_sources: bad=$bad"
[ "$bad" -eq 0 ]
