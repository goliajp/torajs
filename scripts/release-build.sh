#!/usr/bin/env bash
# polish A4 — production release build with build-std +
# panic_immediate_abort. Produces `tr` binaries that, when invoked
# as `tr build user.ts`, ship -90% smaller user binaries vs the
# vanilla `cargo build --release` pipeline.
#
# Why this isn't the default in `.cargo/config.toml`: build-std
# rebuilds core/alloc/std/panic_abort for every cargo invocation,
# including `cargo test`. cargo test needs panic=unwind for
# catch_unwind which conflicts with immediate-abort. Env-var
# gating keeps the size win in production releases without
# breaking the dev/test workflow.
#
# Usage:  scripts/release-build.sh        # default workspace build
#         scripts/release-build.sh tr     # only tr-cli binary
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="${TARGET:-aarch64-apple-darwin}"

# `panic-immediate-abort` is an unstable cargo-features opt-in
# (Rust 1.97+ nightly); pass it via env so we don't have to bake
# it into Cargo.toml's [workspace] (which would break stable).
export CARGO_UNSTABLE_BUILD_STD="core,alloc,std,panic_abort"
export CARGO_UNSTABLE_UNSTABLE_OPTIONS="true"
# `-A linker_messages` silences the nightly lint that surfaces
# Homebrew LLVM's macOS-deployment-target mismatch (libLLVM was
# built for macOS 26.0 but our host targets macOS 11.0); the ABI
# is compatible so this is noise. The same lint is suppressed in
# `.cargo/config.toml` for default-target builds, but RUSTFLAGS
# env var overrides config-level rustflags, so we must re-add it
# here.
export RUSTFLAGS="${RUSTFLAGS:-} -Cpanic=immediate-abort -Zunstable-options -A linker_messages"

cargo build --workspace --release --target "$TARGET" "$@"

# bench-harness + conformance runners hardcode `target/release/tr`
# as the path. Copy the polish-A4-built tr there so downstream tools
# find the small binary. `cp -p` preserves mtime so cargo's stale-
# binary detection doesn't loop-rebuild.
TR_SRC="target/$TARGET/release/tr"
TR_DST="target/release/tr"
if [ -f "$TR_SRC" ]; then
    mkdir -p "$(dirname "$TR_DST")"
    cp -p "$TR_SRC" "$TR_DST"
    echo "polish-A4 tr -> $TR_DST ($(stat -f %z "$TR_DST" 2>/dev/null || stat -c %s "$TR_DST") bytes)"
fi

# Same for conformance + bench binaries — they may be invoked
# directly by ops scripts.
for bin in torajs-conformance bench-harness; do
    SRC="target/$TARGET/release/$bin"
    DST="target/release/$bin"
    [ -f "$SRC" ] && cp -p "$SRC" "$DST"
done
