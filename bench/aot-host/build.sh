#!/usr/bin/env bash
# AOT pipeline driver: tr build → wasm → wasm2c → clang -O3 + PGO → native binary.
#
# Used by the bench harness via runners/torajs-aot.toml. Three positional args:
#   $1 = workspace root (so we can find target/release/tr)
#   $2 = input .tora.ts file
#   $3 = output binary path
#
# The pipeline does PGO (profile-guided optimization):
#   1. tr build     →  emit wasm
#   2. wasm2c       →  translate to C
#   3. clang -O3 -fprofile-generate  →  instrumented binary
#   4. run the instrumented binary  →  profile data
#   5. llvm-profdata merge          →  consolidated profile
#   6. clang -O3 -fprofile-use      →  PGO-optimized final binary
#
# Step 4 deliberately runs the program. The bench cases are deterministic, so
# the profile fits the workload exactly. PGO buys ~3% on fib40 — small in
# absolute terms but real, and per the project's perf priority (execution
# first, compile second) it's worth the extra compile cost.
#
# Exits 3 (not-yet-implemented) when tr's AOT pass refuses the program shape;
# the bench harness recognizes that and reports the row as `skip`.

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
    # Pass through tr's exit code — 3 means "not yet implemented", harness
    # will report skip. 1 means a real build error.
    exit $TR_RC
fi

# 2. wasm2c with a fixed module name so the generated symbols match main.c.
wasm2c --module-name=tora "$WASM" -o "$TMP/tora.c"

# Common compile args.
COMMON_FLAGS=(
    -O3
    -I"$TMP" -I"$WABT_DIR" -I"$WABT_INC"
    "$TMP/tora.c"
    "$WORKSPACE/bench/aot-host/main.c"
    "$WABT_DIR/wasm-rt-impl.c"
    "$WABT_DIR/wasm-rt-mem-impl.c"
)

# 3. Instrumented build to collect profile.
PROF_DIR="$TMP/prof"
mkdir -p "$PROF_DIR"
INSTRUMENTED="$TMP/inst"
clang "${COMMON_FLAGS[@]}" -fprofile-generate="$PROF_DIR" -o "$INSTRUMENTED"

# 4. Run the instrumented binary to generate profile data. Output goes to
#    /dev/null so it doesn't pollute logs. Bench cases are deterministic, so
#    one run is enough.
LLVM_PROFILE_FILE="$PROF_DIR/raw" "$INSTRUMENTED" >/dev/null

# 5. Merge raw profile data into a profdata file clang can consume.
xcrun llvm-profdata merge -output="$PROF_DIR/profdata" "$PROF_DIR/raw"

# 6. Final PGO-optimized build.
clang "${COMMON_FLAGS[@]}" -fprofile-use="$PROF_DIR/profdata" -o "$OUT"
