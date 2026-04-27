# mandelbrot

Sums escape-iteration counts across a 200×200 grid of `(cr, ci)` in the standard Mandelbrot region:

```
cr ∈ [-1.5, 0.5),  ci ∈ [-1.0, 1.0),  max_iter = 1000
total = Σ mandel(cr, ci, 1000)
```

Stresses **tight floating-point loops + function-call overhead**: each inner cell does 4 multiplies, 1 add, 1 sub, 1 compare per iteration; 200² grid × up-to-1000 iter = up to 40M inner iterations.

## Per-language sources

All implementations write the textbook escape-time algorithm directly. f64 throughout (representation matches torajs's `number`).

- `main.tora.ts` — torajs. Uses our dialect's `let`/`while`; lets are hoisted to fn-body top because AOT P3.x doesn't support nested-block lets yet.
- `main.ts` — bun + node, idiomatic `for`-loops + JS expressions.
- `main.py` — Python 3 idiomatic.
- `main.rs` — Rust idiomatic, `f64`/`u32` types.
- `main.go` — Go idiomatic.

## Why `tolerance = 500` in `bench.toml`

Mandelbrot is genuinely FP-precision sensitive — cells right on the set's fractal boundary will flip "escapes / doesn't escape" under any change to operation order, precision, or rounding. Compiler choices about **fused multiply-add (FMA)** are the dominant source of drift here:

- **rust / bun / node / python / torajs-aot via clang**: all give `15383188`. None auto-fuses `a*b + c` into a single rounded FMA without explicit instruction.
- **Go on ARM64**: gives `15382891`. The gc compiler emits `FMADDD` / `FMSUBD` aggressively for any `a*b ± c` pattern — there's no `-ffp-contract=off` knob in Go, and forcing it off via `//go:noinline` barriers slows Go ~2.5× (87 ms vs 35 ms), which would distort the perf comparison.
- **Tora-AOT with `-ffp-contract=fast`**: gives yet a third value (~15383012) because LLVM picks slightly different fusion sites than gc.

We tolerate ±500 — large enough to admit any of those FMA variants, small enough to reject actual algorithmic divergence (a sign flip at a single boundary cell would shift the sum by ~max_iter = 1000).

## Output

A single integer (the sum of escape counts) followed by a newline. The bench harness reads `expected.txt` (`15383188`) and accepts any value within ±500.
