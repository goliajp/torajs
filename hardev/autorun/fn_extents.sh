#!/usr/bin/env bash
# Cross-check for the file-size function audit.
#
# The audit in rules/torajs-file-size-debt.md counts braces character
# by character, so a `{` or `}` inside a string literal or a doc
# comment throws it off and the function reads SHORTER than it is.
# Three functions were over the 200-line limit for months without ever
# appearing in its output (rotation 575).
#
# This script measures the other way: the distance between one
# top-level `fn` and the next. Anything between them that is not the
# first function's body — a struct, a const, a nested `impl` — makes
# this read LONGER than the truth. So:
#
#   audit value <= real length <= this value
#
# When the two agree the number is exact. When they disagree by a lot,
# read the file. Neither is a substitute for the other.
#
# usage: hardev/autorun/fn_extents.sh [threshold]   (default 200)
set -u
THRESHOLD="${1:-200}"
cd "$(git rev-parse --show-toplevel)" || exit 1
find crates -name '*.rs' ! -path '*/target/*' -print0 | xargs -0 awk -v t="$THRESHOLD" '
FNR==1 {
  if (file != "" && prev_start > 0 && prev_lines - prev_start > t)
    printf "%6d  %s:%d  %s\n", prev_lines - prev_start, file, prev_start, prev_name
  file=FILENAME; prev_start=0; prev_name=""
}
{ lines[FILENAME]=FNR }
/^[[:space:]]*(pub(\([a-z]+\))?[[:space:]]+)?(unsafe[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+[a-zA-Z_]/ {
  if (prev_start > 0 && FNR - prev_start > t)
    printf "%6d  %s:%d  %s\n", FNR - prev_start, FILENAME, prev_start, prev_name
  prev_start=FNR; prev_name=$0; sub(/^[[:space:]]*/, "", prev_name); prev_lines=FNR
}
{ prev_lines=FNR }
END {
  if (prev_start > 0 && prev_lines - prev_start > t)
    printf "%6d  %s:%d  %s\n", prev_lines - prev_start, file, prev_start, prev_name
}' | sort -rn
