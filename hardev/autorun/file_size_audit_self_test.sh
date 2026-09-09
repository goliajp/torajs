#!/usr/bin/env bash
#
# hardev autorun pillar — file_size_audit.sh self-test.
#
# Drives every judgement branch from fixtures. The three measurements are
# injected (HARDEV_FILES_OUT / HARDEV_FNS_OUT / HARDEV_EXTENTS_OUT) and the
# baseline comes from a fixture debt file (HARDEV_DEBT_FILE), so no case
# pays for the real awk pass and production state is never read.
#
# Cases:
#   1  everything registered, nothing grew        -> exit 0
#   2  an unregistered over-limit file            -> exit 1, NEW
#   3  a registered file that grew by one         -> exit 1, GREW
#   4  a registered fn that shrank                -> exit 0, ok with -N
#   5  a registered fn that only moved down       -> exit 0 (line is not the key)
#   6  registered, audit silent, fn_extents over  -> exit 0, HIDDEN
#   7  registered, audit silent, both under       -> exit 0, STALE
#   8  a struck-through entry does not cover      -> exit 1, NEW
#   9  prose outside the CARVE-OUT block ignored  -> exit 1, NEW
#  10  debt table unparsable                      -> exit 2
#
# Exit: 0 if all cases behave as expected; 1 otherwise.

set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
AUDIT="$SCRIPT_DIR/file_size_audit.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
pass=0; fail=0

# A debt file with the two headings the parser anchors on. `extra` is
# appended inside the function section, after the CARVE-OUT paragraph.
mk_debt() {
  local files_rows=$1 fns_rows=$2 tail_extra=${3:-}
  {
    echo "### 文件 > 500 行(prod)"
    echo
    echo "| prod | 文件 | 性质 |"
    echo "|---|---|---|"
    printf '%s\n' "$files_rows"
    echo
    echo "### 函数 > 200 行"
    echo
    echo "**已挂 CARVE-OUT #2**(dispatch table):"
    echo '`carve/table.rs:28` 853 / `carve/other.rs:10` 300'
    echo
    echo "| LOC | 位置 |"
    echo "|---|---|"
    printf '%s\n' "$fns_rows"
    echo
    [ -n "$tail_extra" ] && printf '%s\n' "$tail_extra"
    echo "### 下一节"
  } > "$TMP/debt.md"
}

# Ten file rows and ten fn rows: the parser refuses a baseline smaller
# than 3 / 10 (an empty baseline would pass everything), so the fixtures
# must clear that floor before any single case means anything.
FILES_BASE=""
for i in 1 2 3 4 5 6 7 8 9 10; do
  FILES_BASE+="| 60$i | \`c/f$i.rs\` | note |"$'\n'
done
FNS_BASE=""
for i in 1 2 3 4 5 6 7 8 9 10; do
  FNS_BASE+="| 30$i | \`c/g$i.rs:$i\` |"$'\n'
done
FILES_SEEN=""
for i in 1 2 3 4 5 6 7 8 9 10; do
  FILES_SEEN+="60$i	crates/c/f$i.rs"$'\n'
done
FNS_SEEN=""
for i in 1 2 3 4 5 6 7 8 9 10; do
  FNS_SEEN+="30$i	crates/c/g$i.rs:$i"$'\n'
done

run_case() {
  local name=$1 want_exit=$2 want_line=$3 files=$4 fns=$5 extents=${6:-}
  local out rc
  # HARDEV_STAMP_OUT=- : a self-test must not overwrite the real stamp.
  out=$(HARDEV_DEBT_FILE="$TMP/debt.md" HARDEV_FILES_OUT="$files" \
        HARDEV_FNS_OUT="$fns" HARDEV_EXTENTS_OUT="$extents" \
        HARDEV_STAMP_OUT=- bash "$AUDIT" 2>/dev/null)
  rc=$?
  if [ "$rc" -eq "$want_exit" ] && { [ -z "$want_line" ] || printf '%s\n' "$out" | grep -qE "$want_line"; }; then
    printf 'ok   %s\n' "$name"; pass=$(( pass + 1 ))
  else
    printf 'FAIL %s (exit=%d want=%d; wanted /%s/)\n' "$name" "$rc" "$want_exit" "$want_line"
    printf '%s\n' "$out" | sed 's/^/       | /'
    fail=$(( fail + 1 ))
  fi
}

mk_debt "$FILES_BASE" "$FNS_BASE"
run_case "1 all registered"        0 '^--- files_new=0 files_grew=0 fns_new=0 fns_grew=0' "$FILES_SEEN" "$FNS_SEEN"
run_case "2 unregistered file"     1 'NEW +999 +crates/c/zz\.rs' "${FILES_SEEN}999	crates/c/zz.rs" "$FNS_SEEN"
run_case "3 registered file grew"  1 'GREW +602 +crates/c/f1\.rs .*registered 601' "${FILES_SEEN/601	crates\/c\/f1.rs/602	crates/c/f1.rs}" "$FNS_SEEN"
run_case "4 registered fn shrank"  0 'ok +250 +crates/c/g1\.rs:1 .*\(-51\)' "$FILES_SEEN" "${FNS_SEEN/301	crates\/c\/g1.rs:1/250	crates/c/g1.rs:1}"
run_case "5 fn moved, same size"   0 '^--- files_new=0 files_grew=0 fns_new=0 fns_grew=0' "$FILES_SEEN" "${FNS_SEEN/301	crates\/c\/g1.rs:1/301	crates/c/g1.rs:44}"

# 6 / 7 — g1 vanishes from the audit. Whether that is real depends on the
# other ruler, which is the whole point of the cross-check.
FNS_NO_G1="${FNS_SEEN/301	crates\/c\/g1.rs:1$'\n'/}"
run_case "6 audit blind, extents over"  0 'HIDDEN +260 +c/g1\.rs' "$FILES_SEEN" "$FNS_NO_G1" "   260  crates/c/g1.rs:1  fn hidden("
run_case "7 audit blind, both under"    0 'STALE .*c/g1\.rs.*both rulers under' "$FILES_SEEN" "$FNS_NO_G1" "   150  crates/c/g1.rs:1  fn small("

# 8 — a cleared entry, struck through, must not keep covering the path.
mk_debt "$FILES_BASE| 700 | ~~\`c/zz.rs\`~~ | cleared |" "$FNS_BASE"
run_case "8 struck-through not a cover" 1 'NEW +999 +crates/c/zz\.rs' "${FILES_SEEN}999	crates/c/zz.rs" "$FNS_SEEN"

# 9 — narration below the tables ("optimize.rs 516 -> 370+163") reads
# exactly like a CARVE-OUT prose entry. It is not one.
mk_debt "$FILES_BASE" "$FNS_BASE" 'r537 清穿:`c/zz.rs` 999 → 370+163,已除名。'
run_case "9 prose outside carve-out"    1 'NEW +999 +crates/c/zz\.rs' "$FILES_SEEN" "${FNS_SEEN}999	crates/c/zz.rs:5"

# 10 — headings moved: refuse to judge rather than pass everything.
printf '# nothing the parser knows\n' > "$TMP/debt.md"
run_case "10 unparsable debt table"     2 '' "$FILES_SEEN" "$FNS_SEEN"

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
