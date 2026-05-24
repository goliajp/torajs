# torajs-codec-hex performance budgets

Latency budgets enforced informally by `benches/codec_hex.rs` (criterion).
There are no perf-gate tests because hex encode/decode is so cheap that
even pathological input is microseconds — no realistic regression
threshold is meaningful. Budgets here are documentary; criterion
output should match within 2× of dev-machine numbers below.

Run `cargo bench -p torajs-codec-hex` to verify.

## Path taxonomy

`torajs-codec-hex` is a workspace-wide utility used by `torajs-
playground-api` for SHA-256 digest formatting (cache key derivation).
Future F-series crates (F.2 `torajs-hash` etc.) may use it for
identical digest formatting. Single hot shape: 32-byte input →
64-char output (SHA-256 digest hex).

## Budgets

| Path | Budget | Observed (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `encode-32-byte-digest` | ≤ 200 ns | ~60 ns | ~3× | SHA-256 digest hex format. Allocates one 64-byte `String`. Loop body is 4 instructions: shift + index + push high digit + push low digit. |
| `decode-64-char-hex` | ≤ 500 ns | ~150 ns | ~3× | 32-pair loop; each pair = `hex_val × 2` + shift + or + push to `Vec<u8>`. The `hex_val` match table costs the same regardless of valid/invalid input — no shortcut for tight-loop callers. |
| `encode-64-kib` | ≤ 2 ms | ~0.5 ms | 4× | Verifies linear scaling — the per-byte loop runs ~65k times. Allocates one 128 KiB `String`; the `with_capacity` hint avoids any internal reallocation. |

## Algorithmic complexity

| Op | Time | Memory |
| --- | --- | --- |
| `encode` | O(n) per input byte; ~3 ns/byte amortized | 2n + small const String header |
| `decode` | O(n) per output byte; ~5 ns/byte amortized | n + small const Vec header |
| `FromHexError::OddLength` | O(1) | 0 |
| `FromHexError::InvalidHexCharacter` | O(n) worst case; aborts at first non-hex byte | const |

The lookup table for `hex_val` is a `match` arm (`0-9` / `a-f` / `A-F`)
— rustc lowers this to a small range-check + offset rather than a 256-
entry static table. Both shapes are within a couple of ns of each
other on aarch64; no measurable difference.

## What's NOT budgeted

- **Constant-time decode**: out of scope (see README "What's NOT in
  scope"). If a future caller needs side-channel resistance, a
  separate crate is the right move.
- **SIMD acceleration**: a `pshufb`-style table lookup could probably
  hit 5× on encode for inputs ≥ 16 bytes. Not done — the workspace's
  actual hot input is 32 bytes (one SHA-256 digest), which is at the
  edge of where SIMD's setup cost pays back.
- **Streaming / no-alloc encode**: would allow `Write`-impl targets
  to avoid the intermediate `String`. Add when a caller needs it.
