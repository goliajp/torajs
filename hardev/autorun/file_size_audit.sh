#!/usr/bin/env bash
#
# hardev autorun pillar — file / function size audit against the debt table.
#
# `rules/torajs-file-size-debt.md` says, in its own words: "每轮收尾把
# audit 输出逐行对照本表的两张表,不是只看还是那几个老面孔". It was
# not done three rotations running:
#
#   r615  arguments_object_static_argv.rs  490 -> 659, shipped, unnoticed
#   r616  ssa_lower_call_terminal.rs        486 -> 498 (registered margin 2)
#   r617  lexer.rs:34 tokenize_goal         210 -> 220 (registered項只减不增)
#
# Every one of them was missed because "the output looked like it always
# does". That is exactly the comparison a machine does not get bored of.
#
# This script owns the two awk programs (they used to live only as
# copy-paste blocks in the rules file — two spellings of one query is how
# they drift apart), runs them, and diffs the result against the two
# tables in the debt file:
#
#   NEW   an over-limit file/fn that is not registered      -> exit 1
#   GREW  a registered item now larger than its entry       -> exit 1
#   OK    registered and not larger                         -> fine
#   STALE registered but no longer over the limit           -> reported, not fatal
#
# The debt file is the single source of truth for the baseline. If it
# cannot be parsed the script exits 2 — a parser that silently yields an
# empty baseline would pass everything, which is the "instrument fails in
# the shape of clean data" failure this whole pillar exists to prevent
# (see rules/common/file-size.md and perf-decomposition §1).
#
# Two rulers, per lesson 19 of the debt file: the brace count under-reports
# whenever a `{` or `}` sits inside a string literal (`expected \`{{\``),
# so a registered function that the audit stops reporting has NOT
# necessarily shrunk. Every such entry is re-measured with fn_extents.sh
# (fn-start to fn-start = an over-estimate) before it may be called stale:
#
#   audit <= real <= fn_extents
#
# audit says nothing  +  fn_extents still over  ->  HIDDEN (keep the entry)
# audit says nothing  +  fn_extents also under  ->  STALE  (may be removed)
#
# usage:
#   hardev/autorun/file_size_audit.sh            # audit, diff, verdict
#   hardev/autorun/file_size_audit.sh --list     # raw audit output only
#
# exit: 0 clean · 1 NEW or GREW present · 2 internal (debt file unparsable)

set -u

FILE_LIMIT="${HARDEV_FILE_LIMIT:-500}"
FN_LIMIT="${HARDEV_FN_LIMIT:-200}"

ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "file_size_audit: not inside a git repo" >&2; exit 2; }
cd "$ROOT" || exit 2

DEBT="${HARDEV_DEBT_FILE:-$ROOT/.claude/rules/torajs-file-size-debt.md}"
MODE="${1:-audit}"

# ── file face ─────────────────────────────────────────────────────────
# Prod-only line count: an inline `#[cfg(test)] mod tests { .. }` block
# is subtracted (community convention keeps unit tests in the prod file;
# counting them would convict the file for its tests). Files whose first
# line marks them generated are skipped entirely — carve-out #1.
audit_files() {
  find crates -name '*.rs' ! -path '*/target/*' -print0 | xargs -0 awk -v lim="$FILE_LIMIT" '
FNR==1 { if ($0 ~ /CODEGEN|AUTO-GENERATED/) { skip[FILENAME]=1 } in_test=0; depth=0; n[FILENAME]=0 }
skip[FILENAME] { next }
in_test == 0 {
  if ($0 ~ /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*$/) {
    if ((getline nl) > 0 && nl ~ /^[[:space:]]*mod[[:space:]]+tests[[:space:]]*\{[[:space:]]*$/) { in_test=1; depth=1; next }
    else { n[FILENAME]+=2; next }
  }
  n[FILENAME]++; next
}
in_test == 1 {
  for (i=1;i<=length($0);i++){c=substr($0,i,1); if(c=="{")depth++; if(c=="}")depth--}
  if (depth==0) in_test=0
}
END { for (f in n) if (n[f] > lim) printf "%d\t%s\n", n[f], f }' | sort -rn
}

# ── function face ─────────────────────────────────────────────────────
# Brace pairing. Known to UNDER-report when a `{` or `}` appears inside a
# string literal or doc comment (three functions hid over the limit for
# months that way — rotation 575), which is why fn_extents.sh exists as
# the other-direction bound. Under-reporting means this face can miss a
# violation; it can never invent one.
audit_fns() {
  find crates -name '*.rs' ! -path '*/target/*' -print0 | xargs -0 awk -v lim="$FN_LIMIT" '
FNR==1 { in_fn=0; depth=0 }
/^[[:space:]]*(pub(\([a-z]+\))?[[:space:]]+(unsafe[[:space:]]+)?(extern[[:space:]]+"[^"]*"[[:space:]]+)?|unsafe[[:space:]]+|async[[:space:]]+)?fn[[:space:]]+[a-zA-Z_]/ { start=FNR; depth=0; in_fn=1 }
in_fn {
  for (i=1;i<=length($0);i++){ c=substr($0,i,1); if(c=="{")depth++;
    if(c=="}"){ depth--; if(depth==0 && start!=FNR){ if((FNR-start)>lim) printf "%d\t%s:%d\n", (FNR-start), FILENAME, start; in_fn=0; break } } }
}' | sort -rn
}

if [ "$MODE" = "--list" ]; then
  echo "== files > $FILE_LIMIT =="; audit_files
  echo "== fns > $FN_LIMIT ==";     audit_fns
  exit 0
fi

[ -f "$DEBT" ] || { echo "file_size_audit: debt file not found: $DEBT" >&2; exit 2; }

# The three measurements. Overridable so the self-test can drive every
# judgement branch from fixtures instead of paying 5.7s of awk per case
# (same convention as lib.sh's HARDEV_ROTATIONS_LOG / HARDEV_HANDOFF_FILE).
# `${VAR-...}` not `${VAR:-...}`: an injected empty string is a real case
# — it is what "the audit reports nothing" looks like.
FILES_OUT="${HARDEV_FILES_OUT-$(audit_files)}"
FNS_OUT="${HARDEV_FNS_OUT-$(audit_fns)}"
EXTENTS="${HARDEV_EXTENTS_OUT-$(bash "$(dirname "$0")/fn_extents.sh" $(( FN_LIMIT / 2 )) 2>/dev/null)}"

AUDIT_OUT="$(python3 -c "$(cat <<'PY'
import re, sys

debt_path, file_lim, fn_lim, files_raw, fns_raw = sys.argv[1:6]
file_lim, fn_lim = int(file_lim), int(fn_lim)
text = open(debt_path, encoding="utf-8").read()


def section(prefix):
    """Body of the first `### <prefix>...` heading up to the next `###`."""
    m = re.search(r"^###[ \t]*" + re.escape(prefix) + r"[^\n]*\n(.*?)(?=^###[ \t]|\Z)",
                  text, re.S | re.M)
    return m.group(1) if m else ""


def registered(body, prose_only_after=None):
    """[(path, line_or_None, loc)] from both spellings the debt file uses.

    table  |  1428 | `torajs-core/src/ast/ast_def.rs` | ...
    prose  |  `ssa_lower_intrinsics_table.rs:28` 853 / `check/stmt.rs:20` 209

    The prose form is only read inside the block introduced by
    `prose_only_after` (the CARVE-OUT paragraph). Read loosely it also
    swallows narration like "optimize.rs 516 -> 370+163", i.e. entries
    that were cleared, and a cleared entry that keeps counting as
    registered would cover a real regression.

    Struck-through entries (~~...~~) are dropped first for the same reason.
    """
    body = re.sub(r"~~.*?~~", "", body, flags=re.S)
    out = []
    for loc, path in re.findall(r"^\|[ \t]*(\d+)[ \t]*\|[ \t]*`([^`]+)`", body, re.M):
        out.append(_split(path.strip(), int(loc)))
    if prose_only_after:
        m = re.search(re.escape(prose_only_after) + r".*?(?=\n[ \t]*\n)", body, re.S)
        if m:
            for path, loc in re.findall(r"`([^`\s]+\.rs(?::\d+)?)`[ \t]+(\d+)\b", m.group(0)):
                out.append(_split(path.strip(), int(loc)))
    return out


def _split(path, loc):
    """`a/b.rs:28` -> ('a/b.rs', 28, loc); `a/b.rs` -> ('a/b.rs', None, loc)."""
    m = re.match(r"^(.*\.rs):(\d+)$", path)
    return (m.group(1), int(m.group(2)), loc) if m else (path, None, loc)


files_reg = registered(section("文件 > 500"))
fns_reg = registered(section("函数 > 200"), prose_only_after="已挂 CARVE-OUT")

# An empty baseline would pass everything. The debt file has carried
# entries on both faces continuously since it was written; zero means the
# headings moved or the table shape changed, not that the debt is gone.
if len(files_reg) < 3 or len(fns_reg) < 10:
    print(f"file_size_audit: parsed {len(files_reg)} file / {len(fns_reg)} fn entries "
          f"from {debt_path} — the table shape changed, refusing to judge", file=sys.stderr)
    sys.exit(2)


def match(path, line, reg, used):
    """Debt entries are path suffixes ('check/stmt.rs:20'); audit paths are
    repo-relative ('crates/torajs-core/src/check/stmt.rs:20'). Anchor on a
    segment boundary so `stmt.rs` cannot match `parse_stmt.rs`.

    The line number is NOT part of the key. A function that keeps its size
    but moves down four lines is the same debt; keying on `path:line` made
    it read as one NEW violation plus one STALE entry — twice wrong, and
    in the direction that hides the real ones among the noise. Where a
    path carries several registered entries they are paired nearest-first
    by size.
    """
    cands = [(i, e) for i, e in enumerate(reg)
             if i not in used and (path == e[0] or path.endswith("/" + e[0]))]
    if not cands:
        return None
    i, entry = min(cands, key=lambda c: abs(c[1][2] - line))
    used.add(i)
    return entry


def judge(raw, reg, label, limit, extents=None):
    used, new, grew, ok = set(), [], [], []
    for row in raw.splitlines():
        if not row.strip():
            continue
        loc_s, where = row.split("\t", 1)
        loc = int(loc_s)
        path, _, ln = where.partition(":")
        entry = match(path, loc, reg, used)
        if entry is None:
            new.append((loc, where))
        else:
            (grew if loc > entry[2] else ok).append((loc, where, entry))
    print(f"== {label} (limit {limit}) ==")
    for loc, where in new:
        print(f"  NEW    {loc:>5}  {where}")
    for loc, where, e in grew:
        print(f"  GREW   {loc:>5}  {where}   registered {e[2]} (+{loc - e[2]})")
    for loc, where, e in sorted(ok, key=lambda x: -x[0]):
        mark = "=" if loc == e[2] else f"-{e[2] - loc}"
        print(f"  ok     {loc:>5}  {where}   registered {e[2]} ({mark})")
    # Unmatched registrations: the audit stopped reporting them. On the
    # function face that is not evidence of shrinkage (lesson 19), so each
    # one is re-measured with the other ruler before it is called stale.
    for i, (rpath, rline, rloc) in enumerate(reg):
        if i in used:
            continue
        upper = None
        if extents is not None:
            # Debt paths are suffixes; fn_extents emits repo-relative ones.
            upper = max((v for k, v in extents.items()
                         if k == rpath or k.endswith("/" + rpath)), default=None)
        if upper is not None and upper > limit:
            print(f"  HIDDEN {upper:>5}  {rpath}   registered {rloc}; audit blind "
                  f"(brace literal), fn_extents still over — entry kept")
        elif extents is not None:
            print(f"  STALE  {'':>5}  {rpath}   registered {rloc}; both rulers under "
                  f"the limit — may be removed from the table")
        else:
            print(f"  STALE  {'':>5}  {rpath}   registered {rloc}; no longer over the limit")
    return new, grew


# fn_extents.sh measures fn-start to fn-start: an over-estimate, useful
# only as the upper bound of the sandwich. Read at half the limit so a
# registered function that really did shrink still shows a number.
extents = {}
for row in sys.stdin.read().splitlines():
    parts = row.split()
    if len(parts) >= 2 and ":" in parts[1]:
        path = parts[1].rsplit(":", 1)[0]
        extents[path] = max(int(parts[0]), extents.get(path, 0))

f_new, f_grew = judge(files_raw, files_reg, "files", file_lim)
n_new, n_grew = judge(fns_raw, fns_reg, "fns", fn_lim, extents)

bad = len(f_new) + len(f_grew) + len(n_new) + len(n_grew)
print(f"--- files_new={len(f_new)} files_grew={len(f_grew)} "
      f"fns_new={len(n_new)} fns_grew={len(n_grew)}")
if bad:
    print(f"file_size_audit: {bad} violation(s) — register them or bring them back under "
          f"the limit; a registered entry may only shrink", file=sys.stderr)
sys.exit(1 if bad else 0)
PY
)" "$DEBT" "$FILE_LIMIT" "$FN_LIMIT" "$FILES_OUT" "$FNS_OUT" <<< "$EXTENTS")"
RC=$?
printf '%s\n' "$AUDIT_OUT"

# Stamp the run (see write_stamp.sh). Exit 2 — the parser refusing to
# judge — deliberately writes no stamp: a refusal is not a measurement,
# and a stamp saying "ok" because nothing could be compared is precisely
# the instrument-shaped-like-data failure this check exists to prevent.
STAMP_OUT="${HARDEV_STAMP_OUT:-$ROOT/hardev/file-size-latest.json}"
if [ "$STAMP_OUT" != "-" ] && [ "$RC" -lt 2 ]; then
  counts=$(printf '%s\n' "$AUDIT_OUT" | grep -oE '^--- .*$' | tail -1)
  get() { printf '%s\n' "$counts" | grep -oE "$1=[0-9]+" | cut -d= -f2; }
  bash "$(dirname "$0")/write_stamp.sh" "$STAMP_OUT" file_size_audit \
    "$([ "$RC" -eq 0 ] && echo ok || echo fail)" \
    "filesNew=int:$(get files_new)" "filesGrew=int:$(get files_grew)" \
    "fnsNew=int:$(get fns_new)" "fnsGrew=int:$(get fns_grew)" \
    "hidden=int:$(printf '%s\n' "$AUDIT_OUT" | grep -c 'HIDDEN')" \
    "stale=int:$(printf '%s\n' "$AUDIT_OUT" | grep -c 'STALE')" >/dev/null
fi
exit "$RC"
