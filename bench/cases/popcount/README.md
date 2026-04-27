# popcount

`Σ popcount(i)` for `i ∈ [0, 10000000)` using Brian Kernighan's bit-tricks loop:

```
while (n != 0) { n &= n - 1; count++; }
```

Each iteration clears the lowest set bit; the body runs once per set bit. For `i ∈ [0, 10⁷)` the total is `114434624`.

## What this case adds vs the others

A fourth distinct shape:

- **fib40**: deep recursive integer arithmetic.
- **gcd1m**: 1M shallow function calls + integer modulo.
- **mandelbrot**: tight FP nested loops.
- **popcount**: tight integer **bit-ops loop** + 10M function calls.

This case stresses the **integer ALU + branch prediction** path. Each call iterates a small variable-length loop (1–32 iterations depending on input popcount), so branch prediction quality matters. `&` and `-` are the hot operations; `n != 0` is the tight branch.

## Per-language sources

All implementations use the same Kernighan trick. `u64` / `uint64` everywhere; torajs's AOT auto-narrows `number` to `i64` because the program is integer-pure.

- `main.tora.ts` — torajs. `let n` and `let count` hoisted to fn-body top per AOT P3.x's nested-let restriction.
- `main.ts` — bun + node, idiomatic `for` + `++`.
- `main.py` — Python 3.
- `main.rs` — Rust, `u64`.
- `main.go` — Go, `uint64`.

## Output

`114434624` followed by a newline. Byte-exact match (no FMA / FP precision concerns here — pure integer).
