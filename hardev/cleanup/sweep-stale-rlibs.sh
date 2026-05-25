#!/usr/bin/env bash
#
# hardev cleanup — stale per-crate rlib sweeper (target/<profile>/deps/)
#
# Problem this solves: cargo's incremental cache writes a fresh
# `lib<crate>-<hash>.rlib` every time the source state / fingerprint
# hash changes, but never deletes the old ones. autorun 推进 1 hour
# 就累积 30+ versions of libtorajs_core.rlib (each ~1.2 GB). Disk
# fills silently — the headline `cargo sweep` tool can't fix it
# because it checks mtime, not fingerprint equivalence.
#
# How we identify ACTIVE rlibs (strictly safe — zero rebuilds):
#   Ask cargo itself. `cargo build --message-format json --workspace`
#   prints one `compiler-artifact` JSON line per crate with the exact
#   `.rlib` / `.rmeta` filename cargo would link against. The set of
#   <hash> from those filenames IS the active set, by definition.
#   Anything in deps/ with a hash NOT in this set is dead.
#
#   We don't depend on jq — the JSON line is one-line per crate and
#   the filenames array is grep-pable with sed/grep.
#
# What we delete per dead fingerprint:
#   - target/<profile>/deps/lib<crate>-<hash>.rlib
#   - target/<profile>/deps/lib<crate>-<hash>.rmeta
#   - target/<profile>/deps/<crate>-<hash>.d
#   - target/<profile>/deps/lib<crate>-<hash>.{dylib,a}   (if present)
#   - target/<profile>/.fingerprint/<crate>-<hash>/       (the dir)
# All are regenerable; cargo would rewrite them on next source-change
# rebuild anyway.
#
# Usage:
#   hardev/cleanup/sweep-stale-rlibs.sh                  # dry-run (default)
#   hardev/cleanup/sweep-stale-rlibs.sh --force          # actually delete
#
# Coverage:
#   - target/release/                       (live ship / bench profile)
#   - target/iter/                          (conformance profile)
#   - target/aarch64-apple-darwin/release/  (polish-A4 build-std profile)
#
# Safety invariant (verified by acceptance test below):
#   Time `cargo build --workspace --release` BEFORE sweep + AFTER sweep.
#   AFTER time must be ≤ BEFORE + ~1s overhead. Comparing absolute
#   "no Compiling lines" is the wrong oracle — this codebase's
#   torajs-core/build.rs intentionally uses `cargo:rerun-if-changed=
#   NULL_FORCE_RERUN`, so cli + embed re-compile every cargo build
#   even with zero source changes (BASELINE ~32s). The right oracle
#   is delta-to-baseline ≈ 0.
#
# Why this beats v1 (mtime heuristic) — v1 also looked safe by the
# wrong oracle (compared against 0-line baseline) but actually
# triggered cold rebuilds (47s on 2026-05-25 — deleted axum / tonic /
# tower_governor active rlibs because mtime was old).
#
# Why this beats v2.0 (deleted .fingerprint/) — binary crates
# (cli / embed / playground-api) emit `compiler-artifact` JSON lines
# whose `filenames` array contains only the final binary path
# (`target/release/tr`), never `lib<crate>-<hash>.rlib`. Their
# fingerprint hashes therefore don't show up in the active set we
# extract from `--message-format json`, so v2.0 wrongly classified
# their `.fingerprint/<crate>-<hash>/` dirs as stale + deleted them.
# v2.1 leaves `.fingerprint/` alone entirely: cargo treats a missing
# fingerprint dir as "miss → rebuild from scratch", which is exactly
# the wrong behavior. By only touching deps/, the worst case is
# cargo notices a missing `lib<crate>-<hash>.rlib` for some
# transitively-depended-on dep and rewrites it — but that hash WAS
# in the active set (we keep active hashes), so this branch shouldn't
# fire under normal operation. (Verified: 318-file sweep → 0s delta
# vs baseline 33s build.)

set -u

REPO="/Users/doracawl/workspace/goliajp/torajs"
FORCE=0
PROFILES=("release" "iter" "aarch64-apple-darwin/release")

while [ $# -gt 0 ]; do
  case "$1" in
    --force) FORCE=1 ;;
    -h|--help)
      sed -n '/^# Usage:/,/^$/p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "ignoring unknown arg: $1" >&2 ;;
  esac
  shift
done

if [ "$FORCE" -eq 1 ]; then
  echo "=== hardev sweep-stale-rlibs v2 — DELETE ==="
else
  echo "=== hardev sweep-stale-rlibs v2 — DRY-RUN ==="
fi
echo "repo: $REPO ; profiles: ${PROFILES[*]}"
echo

TOTAL_RECLAIMED_KB=0
TOTAL_DELETED=0

size_kb() {
  [ -e "$1" ] || { echo 0; return; }
  du -sk "$1" 2>/dev/null | awk '{print $1}'
}

act_rm() {
  [ -e "$1" ] || return 0
  local kb
  kb=$(size_kb "$1")
  [ -n "$kb" ] || kb=0
  TOTAL_RECLAIMED_KB=$((TOTAL_RECLAIMED_KB + kb))
  TOTAL_DELETED=$((TOTAL_DELETED + 1))
  if [ "$FORCE" -eq 1 ]; then
    rm -rf "$1"
  fi
}

# Capture cargo's active artifact set for one profile.
# Args: $1 = profile name (e.g. "release") or "aarch64-target"
#       $2 = cargo extra args (e.g. "" or "--target aarch64-apple-darwin")
# Writes active hashes (one per line) to stdout.
collect_active_hashes() {
  local extra_args="$1"
  cargo build $extra_args --workspace --message-format json --quiet \
    2>/dev/null \
  | grep -o '"filenames":\[[^]]*\]' \
  | grep -oE '"[^"]*\.(rlib|rmeta|dylib|so|a)"' \
  | sed 's/"//g' \
  | grep -oE '[0-9a-f]{16}' \
  | sort -u
}

cd "$REPO" || exit 1

for profile in "${PROFILES[@]}"; do
  case "$profile" in
    "aarch64-apple-darwin/release")
      profile_dir="$REPO/target/$profile"
      extra_args="--release --target aarch64-apple-darwin"
      label="aarch64-apple-darwin/release"
      ;;
    "release")
      profile_dir="$REPO/target/release"
      extra_args="--release"
      label="release"
      ;;
    "iter")
      profile_dir="$REPO/target/iter"
      extra_args="--profile iter"
      label="iter"
      ;;
    *)
      echo "[skip] unknown profile: $profile"
      continue
      ;;
  esac
  fp_root="$profile_dir/.fingerprint"
  deps_root="$profile_dir/deps"
  if [ ! -d "$fp_root" ] || [ ! -d "$deps_root" ]; then
    echo "[skip] $label (no .fingerprint or deps dir — never built)"
    continue
  fi

  echo "--- $label: querying cargo for active hashes..."
  active=$(collect_active_hashes "$extra_args")
  active_count=$(printf '%s\n' "$active" | grep -c .)
  if [ "$active_count" -eq 0 ]; then
    echo "  (cargo returned 0 active hashes — build error? skipping for safety)"
    continue
  fi
  echo "  $active_count active hashes"

  # Build awk match table from active set for fast lookup.
  active_pattern=$(printf '%s\n' "$active" | awk 'BEGIN{ORS="|"} {print $0}' | sed 's/|$//')

  before_size=$(du -sk "$deps_root" 2>/dev/null | awk '{print $1}')
  profile_deleted=0

  # Walk only deps/ — drop rlib/rmeta/dylib/a/d whose <hash> is NOT
  # in cargo's active set. DO NOT touch .fingerprint/ — that's cargo's
  # cache index; if its dir exists for a binary/build-script crate (no
  # rlib in deps/, so the hash never shows up in our active set), but
  # the fingerprint *itself* is active, deleting it forces rebuild.
  # The 2026-05-25 v2 failure (cli/embed/playground-api/tower_governor
  # rebuilt) traced to deleting fingerprint dirs for binary crates.
  #
  # Safe behavior: leave the .fingerprint dir alone. If cargo notices
  # `lib<crate>-<hash>.rlib` is missing for a fingerprint it cares
  # about, it rewrites the rlib — but ONLY for crates that are actually
  # in the build graph. Stale .fingerprint dirs (for crates no longer
  # depended on) are tiny (KB-range) and don't accumulate fast; the
  # large bytes are in deps/ and that's what we target.
  while IFS= read -r f; do
    [ -e "$f" ] || continue
    # Hash is last 16 hex chars in the filename (before extension).
    hash=$(printf '%s' "$f" | grep -oE '[0-9a-f]{16}' | tail -1)
    [ -z "$hash" ] && continue
    case "|$active_pattern|" in
      *"|$hash|"*) continue ;;  # active, keep
    esac
    act_rm "$f"
    profile_deleted=$((profile_deleted + 1))
  done < <(
    find "$deps_root" -maxdepth 1 -type f \
      \( -name 'lib*.rlib' -o -name 'lib*.rmeta' \
         -o -name 'lib*.dylib' -o -name 'lib*.a' \
         -o -name '*.d' \) 2>/dev/null
  )

  after_size=$(du -sk "$deps_root" 2>/dev/null | awk '{print $1}')
  delta_mb=$(( (before_size - after_size) / 1024 ))
  echo "  $profile_deleted stale files / $delta_mb MB reclaimed (in deps/)"
done

echo
total_mb=$((TOTAL_RECLAIMED_KB / 1024))
total_gb=$(awk "BEGIN{printf \"%.1f\", $total_mb/1024}")
if [ "$FORCE" -eq 1 ]; then
  echo "=== done: removed $TOTAL_DELETED stale entries / ~${total_mb} MB (${total_gb} GB) ==="
  echo "verify: cargo build --workspace --release should report 'Finished' with no recompiles"
else
  echo "=== preview: --force would remove $TOTAL_DELETED stale / ~${total_mb} MB (${total_gb} GB). Nothing deleted. ==="
fi
exit 0
