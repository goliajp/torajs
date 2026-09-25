#!/usr/bin/env bash
#
# hardev autorun pillar — the A-track pre-flight, run on mini from dev
# in one command, with the verdict decided by the script instead of
# by eye.
#
# What it does, in the order the pre-flight checklist fixes (the
# checklist itself: `.claude/runbooks/autorun-edge-cases.md`, "A 轨
# pre-flight checklist"; the rule that points here:
# `rules/torajs-autorun-pipeline.md`, "A 轨 pre-flight"):
#   1. rsync every modified / added / untracked file under crates/ and
#      conformance/cases/ to mini, each at its own full path (never
#      `--relative`: rotation 426).
#   2. On mini, through a login zsh so cargo / bun are on PATH (rotation
#      177): `cargo fmt` for the crates touched; `cargo fmt --all --
#      --check` as a file list (never `head`-truncated: rotation 551);
#      `cargo build --workspace --profile iter` and the four-target
#      release build, each with its OWN exit code and its warning
#      count (rotation 211: a piped `$?` is `tail`'s); `cargo nextest`
#      per touched crate; `staticlib_freshness.sh iter` (rotation 585);
#      every touched fixture — a `.cts` against its `.expected`, a
#      `.ts` against live bun — as EQ / DIFF.
#   3. Take back the files fmt rewrote on mini (`fmt_takeback.sh`, by
#      content).
#   4. `file_size_audit.sh` on dev.
#   5. `PREFLIGHT PASS` or `PREFLIGHT FAIL: <reasons>`; exit 1 on FAIL.
#
# The remote steps are written to a script and scp'd, so every `$?`
# is evaluated on mini (rotation 243). The script name carries a
# timestamp so a stale one can never be picked up (rotation 562).
#
# Usage:
#   preflight_mini.sh [-c crate]... [-F fixture-path]...
#     -c   a crate to fmt / nextest in addition to those inferred from
#          the changed paths (`crates/<name>/...`).
#     -F   a fixture to check in addition to the changed ones.
# shellcheck shell=bash
set -u
. "$(dirname "$0")/lib.sh"

MINI_HOST="${TORAJS_MINI_HOST:-mini}"
MINI_ROOT="${TORAJS_MINI_ROOT:-workspace/goliajp/torajs}"

cd "$PROJECT_DIR" || exit 1

crates=()
fixtures=()
while getopts "c:F:" opt; do
  case "$opt" in
    c) crates+=("$OPTARG") ;;
    F) fixtures+=("$OPTARG") ;;
    *) echo "usage: preflight_mini.sh [-c crate]... [-F fixture]..." >&2; exit 2 ;;
  esac
done

# 1. the changed set, synced file by file.
changed=()
while IFS= read -r f; do
  changed+=("$f")
done < <(git status --short | awk '$1 == "M" || $1 == "A" || $1 == "??" || $1 == "AM" || $1 == "MM" {print $2}' | grep -E '^(crates/|conformance/cases/)')
sources=()
for f in ${changed[@]+"${changed[@]}"}; do
  case "$f" in
    crates/*/*)
      sources+=("$f")
      crate=${f#crates/}; crate=${crate%%/*}
      crates+=("$crate")
      ;;
    conformance/cases/*.cts|conformance/cases/*.ts) fixtures+=("$f") ;;
  esac
  rsync -az --no-times "$f" "$MINI_HOST:$MINI_ROOT/$f"
done
# dedup, sorted, so the remote script is deterministic (bash 3.2 on
# macOS: no mapfile, and an empty array under `set -u` needs the
# `${a[@]+"${a[@]}"}` spelling).
crates_sorted=()
while IFS= read -r c; do crates_sorted+=("$c"); done < <(printf '%s\n' ${crates[@]+"${crates[@]}"} | awk 'NF' | sort -u)
crates=(${crates_sorted[@]+"${crates_sorted[@]}"})
fixtures_sorted=()
while IFS= read -r x; do fixtures_sorted+=("$x"); done < <(printf '%s\n' ${fixtures[@]+"${fixtures[@]}"} | awk 'NF' | sort -u)
fixtures=(${fixtures_sorted[@]+"${fixtures_sorted[@]}"})
echo "preflight: synced ${#changed[@]} file(s); crates: ${crates[*]:-none}; fixtures: ${#fixtures[@]}"

# 2. the remote script.
stamp=$(date +%s)
remote="/tmp/preflight_${stamp}.sh"
local_script="$(mktemp -t preflight)"
{
  echo 'cd ~/'"$MINI_ROOT"' || exit 9'
  if [ ${#crates[@]} -gt 0 ]; then
    printf 'cargo fmt'; for c in "${crates[@]}"; do printf ' -p %s' "$c"; done; echo ' > /dev/null 2>&1'
  fi
  echo 'echo "FMT_DIFF_BEGIN"; cargo fmt --all -- --check 2>&1 | grep "^Diff in" | sed "s/:.*//" | sort -u; echo "FMT_DIFF_END"'
  echo 'cargo build --workspace --profile iter > /tmp/preflight-iter.log 2>&1; echo "ITER_EXIT=$?"; echo "ITER_WARN=$(grep -c "warning:" /tmp/preflight-iter.log)"; grep -E "^error" -A 6 /tmp/preflight-iter.log | head -20'
  echo 'cargo build --workspace --release --lib --bins --examples --tests > /tmp/preflight-rel.log 2>&1; echo "REL_EXIT=$?"; echo "REL_WARN=$(grep -c "warning:" /tmp/preflight-rel.log)"; grep -E "^error" -A 6 /tmp/preflight-rel.log | head -20'
  if [ ${#crates[@]} -gt 0 ]; then
    printf 'cargo nextest run'; for c in "${crates[@]}"; do printf ' -p %s' "$c"; done; echo ' --release 2>&1 | grep -E "Summary|FAIL" | head -5'
  fi
  echo 'bash hardev/autorun/staticlib_freshness.sh iter | tail -1'
  for fx in ${fixtures[@]+"${fixtures[@]}"}; do
    case "$fx" in
      *.cts) echo "diff <(./target/iter/tr run '$fx' 2>/dev/null) '${fx%.cts}.expected' > /dev/null && echo 'FIXTURE EQ $fx' || echo 'FIXTURE DIFF $fx'" ;;
      *.ts)  echo "diff <(./target/iter/tr run '$fx' 2>/dev/null) <(bun '$fx' 2>/dev/null) > /dev/null && echo 'FIXTURE EQ $fx' || echo 'FIXTURE DIFF $fx'" ;;
    esac
  done
} > "$local_script"
scp -q "$local_script" "$MINI_HOST:$remote"
out=$(ssh "$MINI_HOST" "zsh -lc 'bash $remote'" 2>&1)
rm -f "$local_script"
echo "$out"

# 3. fmt take-back, by content.
if [ ${#sources[@]} -gt 0 ]; then
  bash "$AUTORUN_DIR/fmt_takeback.sh" ${sources[@]+"${sources[@]}"}
fi

# 4. dev-side audit.
audit=$(bash "$AUTORUN_DIR/file_size_audit.sh" 2>&1 | tail -1)
echo "$audit"

# 5. verdict.
fails=()
fmt_list=$(printf '%s\n' "$out" | sed -n '/FMT_DIFF_BEGIN/,/FMT_DIFF_END/p' | grep -v "FMT_DIFF_")
[ -n "$fmt_list" ] && fails+=("fmt-check: $fmt_list")
printf '%s\n' "$out" | grep -q '^ITER_EXIT=0$' || fails+=("iter build exit")
printf '%s\n' "$out" | grep -q '^ITER_WARN=0$' || fails+=("iter warnings")
printf '%s\n' "$out" | grep -q '^REL_EXIT=0$' || fails+=("release build exit")
printf '%s\n' "$out" | grep -q '^REL_WARN=0$' || fails+=("release warnings")
if [ ${#crates[@]} -gt 0 ]; then
  printf '%s\n' "$out" | grep -q 'Summary.* 0 failed\|Summary.*passed, 0 skipped\|tests run: [0-9]* passed' || fails+=("nextest")
  printf '%s\n' "$out" | grep -q 'FAIL ' && fails+=("nextest failure lines")
fi
printf '%s\n' "$out" | grep -q 'fails=0' || fails+=("staticlib freshness")
printf '%s\n' "$out" | grep -q '^FIXTURE DIFF' && fails+=("fixture: $(printf '%s\n' "$out" | grep '^FIXTURE DIFF' | tr '\n' ' ')")
printf '%s\n' "$audit" | grep -q 'files_new=0 files_grew=0 fns_new=0 fns_grew=0' || fails+=("file-size audit")
if [ ${#fails[@]} -eq 0 ]; then
  echo "PREFLIGHT PASS"
  exit 0
fi
echo "PREFLIGHT FAIL: $(printf '%s; ' ${fails[@]+"${fails[@]}"})"
exit 1
