#!/usr/bin/env bash
# polish A4.1 (2026-05-24, B2 fix) — production release build with
# build-std + panic=abort but WITHOUT `-Cpanic=immediate-abort`.
#
# # What changed vs the original A4
#
# The original polish A4 (commit df75d25) added
# `-Cpanic=immediate-abort` for a -91% user-binary size win (fib40
# 445 KB → 38 KB). B2 diagnosis (2026-05-24) showed that this flag is
# the ROOT CAUSE of a +19% geomean perf regression across 16/26 bench
# cases: throw-catch-100k +149%, promise-all-1k +86%, async-fn-call
# +43%, fifo-queue +42%, closure-counter +33%. Mechanism: immediate-
# abort rewrites every std panic call site as an inline `abort()`,
# which changes LLVM's hot-path layout / register allocation /
# noreturn modeling away from the cold-call-with-PGO pattern that
# stable+vanilla relies on.
#
# Trade we keep:
#   build-std (rebuild core/alloc/std/panic_abort) → fib40 351 KB
#   (vs 409 KB stable; -14%, perf-neutral)
#
# Trade we drop:
#   immediate-abort → fib40 38 KB (-91% size, but +19% perf cost)
#
# Per CLAUDE.md design pillar #1 (performance-first; run_ms is the
# primary metric) and takagi's 2026-05-24 explicit instruction
# ("我们完全不担心 ROI，只要性能和质量"), perf > size when
# they conflict. Restoring immediate-abort requires a follow-up
# that decouples size win from hot-path layout (e.g. custom panic
# handler without backtrace, opt-level = "z" on cold modules, PGO).
#
# # Why build-std isn't the default in `.cargo/config.toml`
#
# build-std rebuilds core/alloc/std for every cargo invocation,
# including `cargo test`. cargo test needs panic=unwind in
# panic_unwind for catch_unwind, which conflicts with our release
# panic=abort. Env-var gating keeps size win isolated to release
# builds without breaking the dev/test workflow.
#
# Usage:  scripts/release-build.sh        # default workspace build
#         scripts/release-build.sh tr     # only tr-cli binary
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="${TARGET:-aarch64-apple-darwin}"

export CARGO_UNSTABLE_BUILD_STD="core,alloc,std,panic_abort"
export CARGO_UNSTABLE_UNSTABLE_OPTIONS="true"
# `-A linker_messages` silences the nightly lint that surfaces
# Homebrew LLVM's macOS-deployment-target mismatch (libLLVM was
# built for macOS 26.0 but our host targets macOS 11.0); the ABI
# is compatible so this is noise. The same lint is suppressed in
# `.cargo/config.toml` for default-target builds, but RUSTFLAGS
# env var overrides config-level rustflags, so we must re-add it
# here.
#
# IMPORTANT — `-Cpanic=immediate-abort` deliberately omitted (see
# the A4.1 header above). DO NOT re-add without a B2-follow-up
# benchmark proving the perf cost has been recovered.
export RUSTFLAGS="${RUSTFLAGS:-} -Zunstable-options -A linker_messages"

cargo build --workspace --release --target "$TARGET" "$@"

# bench-harness + conformance runners hardcode `target/release/tr`
# as the path. Copy the polish-A4.1-built tr there so downstream
# tools find the build-std artifact. `cp -p` preserves mtime so
# cargo's stale-binary detection doesn't loop-rebuild.
TR_SRC="target/$TARGET/release/tr"
TR_DST="target/release/tr"
if [ -f "$TR_SRC" ]; then
    mkdir -p "$(dirname "$TR_DST")"
    cp -p "$TR_SRC" "$TR_DST"
    echo "polish-A4.1 tr -> $TR_DST ($(stat -f %z "$TR_DST" 2>/dev/null || stat -c %s "$TR_DST") bytes)"
fi

# Same for conformance + bench binaries — they may be invoked
# directly by ops scripts.
for bin in torajs-conformance bench-harness; do
    SRC="target/$TARGET/release/$bin"
    DST="target/release/$bin"
    [ -f "$SRC" ] && cp -p "$SRC" "$DST"
done
