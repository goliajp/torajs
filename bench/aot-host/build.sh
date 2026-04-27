#!/usr/bin/env bash
# AOT pipeline driver: tr build → wasm → wasm2c → clang -O3 → native binary.
#
# Used by the bench harness via runners/torajs-aot.toml. Three positional args:
#   $1 = workspace root (so we can find target/release/tr)
#   $2 = input .tora.ts file
#   $3 = output binary path
#
# Per-build cost is minimized by **precompiling the runtime once** into
# `target/aot-cache/libtorart.a`. The archive contains:
#   - wabt's wasm-rt-impl.o + wasm-rt-mem-impl.o (the WASI sandbox runtime)
#   - our bench/aot-host/main.o (the host glue: fd_write impl + main())
# These are identical for every program we compile, so amortizing them saves
# ~120 ms per build on this hardware. Per-build only compiles the user's
# wasm2c-generated tora.c + links against the cached archive.
#
# Cache invalidation: bump (or rm) the cache directory whenever wabt is
# updated, clang is updated, or `bench/aot-host/main.c` changes. We check
# none of these automatically — keep it simple, document the rule.
#
# We do NOT do PGO. See the comment block in this script's git history;
# PGO regressed popcount 2× because clang's loop-idiom recognition for
# Brian Kernighan's popcount → ARM `cnt.16b` got disabled by PGO's branch
# data. fib40's modest PGO gain didn't justify popcount's loss.
#
# Exits 3 (not-yet-implemented) when tr's AOT pass refuses the program shape.

set -u

WORKSPACE="$1"
SRC="$2"
OUT="$3"

WABT_PREFIX="$(brew --prefix wabt 2>/dev/null)"
WABT_DIR="$WABT_PREFIX/share/wabt/wasm2c"
WABT_INC="$WABT_PREFIX/include"
if [ ! -d "$WABT_DIR" ]; then
    echo "wabt not found via homebrew; install with \`brew install wabt\`" >&2
    exit 1
fi

CACHE_DIR="$WORKSPACE/target/aot-cache"
LIBTORART="$CACHE_DIR/libtorart.a"

# Build the runtime archive once.  Uses a tiny sentinel program to extract
# the canonical tora.h that wasm2c always emits for our (fixed) wasm shape:
# imports `wasi_snapshot_preview1.fd_write`, exports `memory` + `_start`.
# main.c's struct-layout assumptions about `w2c_tora` are baked into main.o
# at this point; subsequent user programs that fit the same shape link
# against it and get matching offsets.
if [ ! -f "$LIBTORART" ]; then
    mkdir -p "$CACHE_DIR"
    SENTINEL_TS="$CACHE_DIR/sentinel.tora.ts"
    echo 'console.log("x")' > "$SENTINEL_TS"
    "$WORKSPACE/target/release/tr" build "$SENTINEL_TS" -o "$CACHE_DIR/sentinel.wasm"
    wasm2c --module-name=tora "$CACHE_DIR/sentinel.wasm" -o "$CACHE_DIR/tora.c"

    clang -O3 -c -I"$CACHE_DIR" -I"$WABT_DIR" -I"$WABT_INC" \
        "$WORKSPACE/bench/aot-host/main.c" -o "$CACHE_DIR/main.o"
    clang -O3 -c -I"$WABT_DIR" -I"$WABT_INC" \
        "$WABT_DIR/wasm-rt-impl.c" -o "$CACHE_DIR/wasm-rt-impl.o"
    clang -O3 -c -I"$WABT_DIR" -I"$WABT_INC" \
        "$WABT_DIR/wasm-rt-mem-impl.c" -o "$CACHE_DIR/wasm-rt-mem-impl.o"

    ar rcs "$LIBTORART" \
        "$CACHE_DIR/main.o" \
        "$CACHE_DIR/wasm-rt-impl.o" \
        "$CACHE_DIR/wasm-rt-mem-impl.o"
    rm "$CACHE_DIR/main.o" "$CACHE_DIR/wasm-rt-impl.o" \
       "$CACHE_DIR/wasm-rt-mem-impl.o" \
       "$CACHE_DIR/sentinel.wasm" "$CACHE_DIR/tora.c" \
       "$CACHE_DIR/tora.h" "$SENTINEL_TS"
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

WASM="$TMP/m.wasm"

# 1. Emit wasm via tr.
"$WORKSPACE/target/release/tr" build "$SRC" -o "$WASM"
TR_RC=$?
if [ $TR_RC -ne 0 ]; then
    exit $TR_RC
fi

# 2. wasm2c with a fixed module name so the generated symbols match main.c.
wasm2c --module-name=tora "$WASM" -o "$TMP/tora.c"

# 3. clang: compile only the user's tora.c, link against the precompiled
#    archive (main.o + wasm-rt + wasm-rt-mem).
#
# Optimization level is per-case configurable via TORAJS_AOT_CLANG_FLAGS
# (set by the bench harness from each case's `bench.toml: aot_clang_flags`).
# Default is `-O3`. Empirically the optimal level is workload-dependent:
#   fib40, startup        — `-O1`  (-O2/3's loop transforms hurt these)
#   popcount              — `-O3`  (needs LLVM's loop-idiom → ARM `cnt.16b`)
#   mandelbrot, gcd1m     — any   (within noise across -O1..-O3)
CLANG_FLAGS="${TORAJS_AOT_CLANG_FLAGS:--O3}"
clang $CLANG_FLAGS -I"$TMP" -I"$WABT_DIR" -I"$WABT_INC" \
    "$TMP/tora.c" "$LIBTORART" \
    -o "$OUT"
