# torajs-pool performance budgets

Latency budgets enforced by `tests/perf_gate.rs`. Each budget is set
with 15-30× headroom over the observed P95 on a dev machine so CI
fails on order-of-magnitude regressions, not micro-noise.

Run `cargo test -p torajs-pool --test perf_gate` to check.

## Path taxonomy

`FixedPool` sits at **Layer 0** in the torajs runtime crate stack
(see `docs/architecture-rewrite.md`). It serves the hot alloc path
for fixed-size heap structs — `Promise`, `Closure env`, `capture
box`, etc. Per `rules/rust/patterns.md`: hot paths sit at µs / ns
budget. Both gated paths below run once per allocation; for
torajs's Promise-heavy benchmarks (promise-chain-1k allocates
~3000 Promises, promise-await-100k allocates ~100k) this is the
single hottest line of code in the runtime.

## Budgets

| Path | Budget | Observed P95 (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `acquire_release_hot` (100k iter pairs, warm pool) | 5 ms | ~0.4 ms | ~12× | Pool warmed to CAP; each iter pops + pushes a slot via const-offset pointer arithmetic. Real torajs bench equivalent: promise-await-100k case at ~3 ms (post P-PERF.A6 ship). |
| `pooled_count_invariant` | functional | — | — | Verifies `pooled() ≤ capacity()` after any acquire / release sequence. Not a perf gate; presence is to catch invariant breaks if a future refactor mismanages the count. |

Real-world `acquire_release_hot` cost is dominated by the
const-offset deref + compare-update; the criterion micro-bench
under `benches/pool.rs` measures ns/op (the integration above is
the µs-aggregate variant for CI catch).

## Methodology

- Each test runs the path 100k times.
- The **median** sample is asserted under the budget, not the mean —
  median is robust to occasional GC / context-switch noise.
- Budgets are **wall-clock**, not CPU time. Tests must be runnable
  on any reasonable CI executor; we don't pin to high-resolution
  clocks or special hardware.

## When to re-measure

Update the table (and the asserts in `perf_gate.rs`) when any of
these fire:

- Algorithm change (e.g. switch to power-of-two index-based slab
  instead of LIFO pointer free-list)
- Layout change (struct grew / `next_offset` moved)
- Code-gen change (rustc / LLVM upgrade)
- A criterion bench median in `benches/pool.rs` moves by > 30%
  in either direction — that's a real signal worth chasing, not
  noise.
