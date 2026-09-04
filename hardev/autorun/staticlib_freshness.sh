#!/usr/bin/env bash
#
# hardev autorun pillar — is every runtime staticlib baked into `tr`
# actually built from the source that is on disk right now?
#
# Why this exists:
#   `tr` embeds 36 `libtorajs_*.a` archives through `include_bytes!`
#   in `torajs-core/src/staticlibs.rs`. Cargo does track those paths
#   (they appear in `target/<profile>/deps/torajs_core-*.d`), so a
#   `--workspace` build propagates a runtime edit into `tr` in one
#   step — measured rotation 585, and the standing advice to touch
#   `core/lib.rs` and build twice buys nothing.
#
#   What it does not survive is a `-p` scoped build, and the reason is
#   cargo's uplift. The path `include_bytes!` reads —
#   `target/<profile>/lib<name>.a` — is a copy cargo lifts out of
#   `deps/` for the packages it was ASKED to build. Build
#   `-p torajs-cli` and the runtime crate is still compiled (rustc runs
#   with `--crate-type staticlib`, the archive lands in
#   `deps/lib<name>-<hash>.a`) but the lifted copy is left alone.
#   Measured rotation 585 on mini: after editing `torajs-fnname` that
#   way, `strings target/iter/libtorajs_fnname.a | grep -c EDGE2`
#   answered 0 with the fresh `deps/libtorajs_fnname-<hash>.a` sitting
#   next to it. This holds for all 36 archives; a dependency edge does
#   not change it, which was measured the same rotation by adding one.
#
#   The dangerous part is what such a build leaves behind: it relinks
#   `tr`. A fresh mtime over two-day-old runtime bytes defeats every
#   "is my binary new?" heuristic in the rules, all of which read that
#   mtime. Rotation 584 lost ten fixtures to it — the multi-line native
#   `toString` went into `torajs-fnname` on 2026-09-03 and the gate
#   stayed green until something unrelated forced a re-lift.
#
#   Conformance is structurally blind here — it runs whatever `tr` it
#   is handed and has no way to ask whether that `tr` agrees with the
#   working tree. So is `cargo`, which is right to say nothing: it did
#   what it was asked. This check is the missing question, and it
#   belongs beside `build_determinism.sh` and `gmalloc_scan.sh` — the
#   gates for what all the other gates cannot see.
#
#   The orthodox fix would be cargo's artifact dependencies
#   (`-Z bindeps`), which hand the consumer the real path instead of a
#   lifted copy. Not available here: nightly-2026-07-10's cargo answers
#   `unknown Cargo.toml feature \`bindeps\`` to every placement tried.
#
# Usage:
#   staticlib_freshness.sh [profile]      # profile default: iter
#
# Behaviour:
#   - Reads the archive list from `torajs-core/build.rs`'s STATICLIBS
#     const, so a crate added there is checked without editing this
#     script (and a crate added there with no archive on disk fails
#     loudly rather than silently embedding nothing).
#   - For each: the archive must exist and be no older than the newest
#     of that crate's `src/**/*.rs`, `Cargo.toml` and `build.rs`.
#   - `tr` must be no older than every archive it bakes.
#   - Emits CSV on stdout: `lib,status,detail`. Exit 1 on any FAIL.
#
# Note on `git checkout` / `rsync`: both stamp a source file with the
# current time, so a restored tree reads as STALE until it is rebuilt.
# That is the intended answer — the binary really does not match the
# tree yet.
#
# Exit codes: 0 = everything fresh, 1 = at least one FAIL, 2 = usage
set -uo pipefail

profile="${1:-iter}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root" || exit 2

build_rs="crates/torajs-core/build.rs"
[ -f "$build_rs" ] || { echo "no $build_rs" >&2; exit 2; }

target_dir="$(cargo metadata --no-deps --format-version 1 2>/dev/null \
  | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)"
[ -n "$target_dir" ] || { echo "cannot resolve target_directory" >&2; exit 2; }
prof_dir="$target_dir/$profile"

mtime() { stat -f '%m' "$1" 2>/dev/null || echo 0; }

# newest mtime among a crate's build inputs
newest_src() {
  local dir="$1" newest=0 t
  for f in $(find "$dir/src" -name '*.rs' 2>/dev/null) "$dir/Cargo.toml" "$dir/build.rs"; do
    [ -f "$f" ] || continue
    t="$(mtime "$f")"
    [ "$t" -gt "$newest" ] && newest="$t"
  done
  echo "$newest"
}

fails=0
checked=0
newest_a=0

echo "lib,status,detail"
for name in $(grep -oE '^    "torajs_[a-z_0-9]+"' "$build_rs" | tr -d ' "'); do
  checked=$((checked + 1))
  crate_dir="crates/$(echo "$name" | tr '_' '-')"
  archive="$prof_dir/lib$name.a"

  if [ ! -d "$crate_dir" ]; then
    echo "$name,FAIL,no crate dir $crate_dir"
    fails=$((fails + 1))
    continue
  fi
  if [ ! -f "$archive" ]; then
    echo "$name,FAIL,archive missing: $archive"
    fails=$((fails + 1))
    continue
  fi

  a_t="$(mtime "$archive")"
  s_t="$(newest_src "$crate_dir")"
  [ "$a_t" -gt "$newest_a" ] && newest_a="$a_t"

  if [ "$s_t" -gt "$a_t" ]; then
    echo "$name,FAIL,source is $((s_t - a_t))s newer than archive"
    fails=$((fails + 1))
  else
    echo "$name,ok,archive $((a_t - s_t))s newer than source"
  fi
done

tr_bin="$prof_dir/tr"
if [ ! -f "$tr_bin" ]; then
  echo "tr,FAIL,binary missing: $tr_bin"
  fails=$((fails + 1))
else
  tr_t="$(mtime "$tr_bin")"
  if [ "$newest_a" -gt "$tr_t" ]; then
    echo "tr,FAIL,an archive is $((newest_a - tr_t))s newer than tr — tr bakes stale bytes"
    fails=$((fails + 1))
  else
    echo "tr,ok,newer than every archive it bakes"
  fi
fi

echo "--- profile=$profile libs=$checked fails=$fails"
[ "$fails" -eq 0 ] || exit 1
