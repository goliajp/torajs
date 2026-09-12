#!/usr/bin/env bash
#
# hardev autorun pillar — take back from mini every file that
# `cargo fmt` rewrote there, judged by content, not by git.
#
# Why this exists:
#   The dev↔mini workflow sends sources by rsync and formats on mini
#   (the pre-flight runs `cargo fmt` there). A file fmt rewrote must
#   come back before the commit, or the committed version is not
#   fmt-clean and the next `cargo fmt --check` gate goes red
#   (rotation 428 / 551 / 639 all shipped one that way).
#
#   `git status --short` on mini cannot answer "which files did fmt
#   touch": mini's tree is rsync-fed and its git HEAD trails by dozens
#   of commits, so every synced file shows `M` there — rotation 639
#   measured 300+ rows for a 14-file change. The only reliable judge
#   is CONTENT: a file whose md5 differs between dev and mini after
#   fmt ran on mini is one fmt changed (everything else was rsync'd
#   from dev byte for byte).
#
# Usage:
#   fmt_takeback.sh [file ...]
#     Files are project-relative paths. With no arguments the list is
#     every modified / added / untracked file under crates/ that
#     `git status --short` reports on dev — the set the pre-flight
#     synced.
#
# Output:
#   one `takeback: <file>` line per file brought back, then
#   `fmt_takeback: N file(s) taken back`. Exit 0 always — a file that
#   is missing on mini is reported (`missing-on-mini: <file>`) and
#   skipped, never guessed.
# shellcheck shell=bash
set -u
. "$(dirname "$0")/lib.sh"

MINI_HOST="${TORAJS_MINI_HOST:-mini}"
MINI_ROOT="${TORAJS_MINI_ROOT:-workspace/goliajp/torajs}"

cd "$PROJECT_DIR" || exit 1

files=("$@")
if [ ${#files[@]} -eq 0 ]; then
  while IFS= read -r f; do
    files+=("$f")
  done < <(git status --short | awk '$1 == "M" || $1 == "A" || $1 == "??" || $1 == "AM" || $1 == "MM" {print $2}' | grep '^crates/')
fi
if [ ${#files[@]} -eq 0 ]; then
  echo "fmt_takeback: 0 file(s) taken back (nothing to compare)"
  exit 0
fi

# One ssh round trip for every md5: the remote prints `<md5>  <path>`
# per file, `MISSING <path>` for one that is not there.
remote_script='cd "$1"; shift; for f in "$@"; do if [ -f "$f" ]; then printf "%s %s\n" "$(md5 -q "$f")" "$f"; else printf "MISSING %s\n" "$f"; fi; done'
remote_out=$(ssh "$MINI_HOST" "bash -c '$remote_script' _ \"$MINI_ROOT\" $(printf '"%s" ' "${files[@]}")")

taken=0
while IFS= read -r line; do
  [ -z "$line" ] && continue
  rsum=${line%% *}
  f=${line#* }
  if [ "$rsum" = "MISSING" ]; then
    echo "missing-on-mini: $f"
    continue
  fi
  lsum=$(md5 -q "$f" 2>/dev/null || echo "-")
  if [ "$lsum" != "$rsum" ]; then
    rsync -az --no-times "$MINI_HOST:$MINI_ROOT/$f" "$f"
    echo "takeback: $f"
    taken=$((taken + 1))
  fi
done <<< "$remote_out"
echo "fmt_takeback: $taken file(s) taken back"
exit 0
