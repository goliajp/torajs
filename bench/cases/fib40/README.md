# fib40

`fib(40)` via the textbook recursive definition (no memoization). Stresses **function-call overhead**: 2^40 ≈ 10¹² operations is what an AOT-friendly machine would face, but the call-tree balloons quickly so what we actually measure is the runtime's per-call cost.

Expected stdout: `102334155\n`.

## Per-language sources

All five non-torajs implementations are intentionally **textbook recursive**, no tail-call tricks, no memoization, no iterative rewrite. The point is to compare the runtime's machinery, not who can recognize fibonacci.

- `main.ts` — `function fib(n: number): number` recursion (bun, node)
- `main.tora.ts` — same shape on torajs (currently the tree-walk interpreter; AOT comes in P3)
- `main.py` — `def fib(n)` with the same recursive body
- `main.rs` — `fn fib(n: u64) -> u64`
- `main.go` — `func fib(n int) int`

## Why the tuned `bench.toml`

torajs's interpreter tree-walks every call frame in Rust; fib(40) makes 2 × `fib(40)` − 1 ≈ 330 million calls. With a default `--warmup 3 --runs 10` hyperfine budget, torajs alone would dominate the bench at ~3.7 minutes. We trim to `--warmup 1 --runs 3`, accepting wider error bars in exchange for a ~3 minute total bench across all six runtimes.

When P3 (AOT to wasm) lands, revisit and probably restore the default budget — torajs should drop into the sub-100 ms range like rust/go.
