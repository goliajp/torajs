#!/usr/bin/env bash
#
# torajs dev-env stale-file cleaner  (.dev theme)
#
# Removes ONLY regenerable / pure-scratch artifacts. Never touches
# source / committed / .git / non-torajs paths / shared infra. Dry-run
# is the DEFAULT: without --force it only prints what it would remove
# and how much it would reclaim, deleting nothing.
#
#   .dev/clean.sh                         # dry-run (default, safe)
#   .dev/clean.sh --force                 # actually delete
#   .dev/clean.sh --force --keep-logs 5   # keep newest N log files (default 3)
#
# Principles (aligned with CLAUDE.md Disk Hygiene HARD RULE +
# classic-errors): verify-before-delete (du -sh big dirs); macOS /tmp
# is a symlink to /private/tmp so use the real path (cross-symlink
# find -delete fails silently); never `cargo clean` (shared-target
# wipe incident, cargo-target-dir.md §7); never touch .nudo.toml,
# ~/.cargo-target-fallback, or target/release. Add a grep-able glob
# rule per new enumerable stale source; keep dry-run the default.
#
set -u

REPO="/Users/doracawl/workspace/goliajp/torajs"
FORCE=0
KEEP_LOGS=3

while [ $# -gt 0 ]; do
  case "$1" in
    --force)     FORCE=1 ;;
    --keep-logs) shift; KEEP_LOGS="${1:-3}" ;;
    *)           echo "ignoring unknown arg: $1" >&2 ;;
  esac
  shift
done
case "$KEEP_LOGS" in (*[!0-9]*|'') KEEP_LOGS=3 ;; esac

if [ "$FORCE" -eq 1 ]; then
  echo "=== torajs .dev/clean.sh — DELETE ==="
else
  echo "=== torajs .dev/clean.sh — DRY-RUN (no --force; preview only) ==="
fi
echo "repo: $REPO ; keep newest $KEEP_LOGS logs per family"
echo

RECLAIMED_KB=0
keep_plus_one=$((KEEP_LOGS + 1))

size_kb() { du -sk "$1" 2>/dev/null | awk '{print $1}'; }

act_rm() {
  [ -e "$1" ] || return 0
  local kb
  kb=$(size_kb "$1")
  [ -n "$kb" ] || kb=0
  RECLAIMED_KB=$((RECLAIMED_KB + kb))
  if [ "$FORCE" -eq 1 ]; then
    rm -rf "$1" && echo "  removed  (${kb} KB)  $1"
  else
    echo "  would-rm (${kb} KB)  $1"
  fi
}

# 1. /private/tmp scratch *.ts (per-session probe/fixture drafts)
echo "[1] /private/tmp scratch *.ts"
while IFS= read -r f; do act_rm "$f"; done < <(
  find /private/tmp -maxdepth 1 -type f -name '*.ts' 2>/dev/null)

# 2. /private/tmp tr-build leftovers: *.dSYM dirs + bare *.bin
echo "[2] /private/tmp *.dSYM + *.bin scratch"
while IFS= read -r f; do act_rm "$f"; done < <(
  find /private/tmp -maxdepth 1 \( -name '*.dSYM' -o -name '*.bin' \) 2>/dev/null)

# 3. torajs log families — keep newest KEEP_LOGS, drop older
echo "[3] /private/tmp torajs-* logs (keep newest $KEEP_LOGS)"
for pat in 'torajs-conformance-*.log' 'torajs-conf-*.log' \
           'torajs-bench-*.log' 'torajs-5k-*.log' \
           'torajs-conf-t13*.log' 'full-dump*.txt'; do
  while IFS= read -r f; do
    [ -n "$f" ] && act_rm "$f"
  done < <(ls -t /private/tmp/$pat 2>/dev/null | tail -n "+${keep_plus_one}")
done

# 4. repo bun-build caches (bench --compile drops .<HEX>-<N>.bun-build;
#    .gitignore stops commit, not disk use)
echo "[4] repo *.bun-build caches"
while IFS= read -r f; do act_rm "$f"; done < <(
  find "$REPO" -maxdepth 4 -name '*.bun-build' 2>/dev/null)

# 5. project-private target waste subtrees (workflow is --release only;
#    debug/doc are pure waste). NEVER rm target/release (live cache),
#    NEVER cargo clean, NEVER touch ~/.cargo-target-fallback.
echo "[5] <repo>/target/{debug,doc} waste (release kept)"
for sub in debug doc; do
  [ -d "$REPO/target/$sub" ] && act_rm "$REPO/target/$sub"
done

# explicit never-touch guard (warn if present, never act)
for guard in "$REPO/.nudo.toml" "$HOME/.cargo-target-fallback" \
             "$REPO/target/release"; do
  [ -e "$guard" ] && echo "[skip] never cleaned (not-stale/shared/live): $guard"
done

echo
RECLAIMED_MB=$((RECLAIMED_KB / 1024))
if [ "$FORCE" -eq 1 ]; then
  echo "=== done: reclaimed ~${RECLAIMED_MB} MB (du magnitude; APFS may differ) ==="
else
  echo "=== preview done: --force would reclaim ~${RECLAIMED_MB} MB. Nothing deleted. ==="
fi
exit 0
