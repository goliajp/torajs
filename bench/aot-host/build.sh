#!/usr/bin/env bash
# AOT pipeline driver: tr build → wasm → wasm2c → clang -O3 → native binary.
#
# Used by the bench harness via runners/torajs-aot.toml. Three positional args:
#   $1 = workspace root (so we can find target/release/tr)
#   $2 = input .tora.ts file
#   $3 = output binary path
#
# Exits 3 (not-yet-implemented) when tr's AOT pass refuses the program shape;
# the bench harness recognizes that and reports the row as `skip`.

set -u

WORKSPACE="$1"
SRC="$2"
OUT="$3"

WABT_DIR="$(brew --prefix wabt 2>/dev/null)/share/wabt/wasm2c"
WABT_INC="$(brew --prefix wabt 2>/dev/null)/include"
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

# 3. clang -O3 — the wasm-rt-impl + tora.c + our main.c → native binary.
clang -O3 \
    -I"$TMP" -I"$WABT_DIR" -I"$WABT_INC" \
    "$TMP/tora.c" \
    "$WORKSPACE/bench/aot-host/main.c" \
    "$WABT_DIR/wasm-rt-impl.c" \
    "$WABT_DIR/wasm-rt-mem-impl.c" \
    -o "$OUT"
