#!/usr/bin/env bash
# AOT pipeline driver: tr build → wasm → wasm2c → clang -O3 → native binary.
#
# Used by the bench harness via runners/torajs-aot.toml. Three positional args:
#   $1 = workspace root (so we can find target/release/tr)
#   $2 = input .tora.ts file
#   $3 = output binary path
#
# We do NOT do PGO.  Empirical finding (2026-04-28): PGO's instrumented profile
# run can push branch-frequency data that makes clang -O3 disable certain loop
# idiom recognitions (notably Brian Kernighan's popcount → ARM NEON `cnt.16b`).
# Net across the current bench cases:
#
#   workload     run_ms with PGO    run_ms without PGO    delta
#   fib40        ~160               ~165                  +5  (PGO win)
#   gcd1m        ~40                ~40                    0
#   mandelbrot   ~35                ~35                    0
#   popcount     ~5.6               ~2.8                  -2.8 (PGO loss, 2x)
#   startup      ~1.3               ~1.3                   0
#
# popcount's 50% regression outweighs fib40's 3% gain, so the net effect of
# PGO across the bench is negative on this hardware. The right principle is
# perf-priority (run_ms first); if a single optimization regresses ANY case
# substantially, it doesn't belong in the default pipeline.
#
# We may add an opt-in per-case PGO knob later (`bench.toml: pgo = true`),
# but the default stays clang -O3 only.
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

# 3. clang -O3 — single pass. wasm-rt-impl + tora.c + our main.c → native.
clang -O3 \
    -I"$TMP" -I"$WABT_DIR" -I"$WABT_INC" \
    "$TMP/tora.c" \
    "$WORKSPACE/bench/aot-host/main.c" \
    "$WABT_DIR/wasm-rt-impl.c" \
    "$WABT_DIR/wasm-rt-mem-impl.c" \
    -o "$OUT"
