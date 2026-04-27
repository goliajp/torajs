# gcd1m

Computes `Σ gcd(i, 1234567)` for i ∈ [1, 1000000]. Stresses **integer modulo + tight conditional loops + 1M function calls**.

`1234567 = 127 × 9721` so most `i` give `gcd = 1`, multiples of 127 give 127, multiples of 9721 give 9721, and i = 1234567 itself isn't in range. Exact total: `2983564`.

## Per-language sources

All implementations write a textbook iterative Euclid GCD. Integer types throughout — `i64`/`u64` in compiled languages, `number` (which torajs's AOT narrows to `i64` because there's no `/` and no fractional literal anywhere). f64 representation in tr's interpreter is lossless for these magnitudes.

- `main.tora.ts` — torajs. The `let t` is hoisted to fn-body top (AOT can't yet declare locals inside while bodies; same caveat as mandelbrot).
- `main.ts` — bun + node, idiomatic `for`-loop.
- `main.py` — Python 3 idiomatic.
- `main.rs` — Rust, `u64`.
- `main.go` — Go, `uint64`.

## What this case adds vs fib40 / mandelbrot

Different shape from the other two:

- **fib40**: deep recursion, branchy, integer arithmetic.
- **mandelbrot**: shallow tight loops over FP, predictable branches.
- **gcd1m**: **1M function calls** (vs fib's 165M, but each call does many fewer ops), modulo as the hot op, branch-on-zero exit.

Modulo is interesting because it's relatively expensive (~10× faster than div on most ISAs, but still way slower than add/mul). Also, gcd's loop body is short and the call overhead is a non-trivial fraction of total work — exposes function-call codegen quality.
