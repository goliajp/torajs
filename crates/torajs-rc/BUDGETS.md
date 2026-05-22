# torajs-rc performance budgets

Latency budgets enforced by `tests/perf_gate.rs`. Each budget is set
with 15-30× headroom over the observed P95 on a dev machine so CI
fails on order-of-magnitude regressions, not micro-noise.

Run `cargo test -p torajs-rc --test perf_gate` to check.

## Path taxonomy

`torajs-rc` sits at **Layer 1** in the torajs runtime crate stack
(see `docs/architecture-rewrite.md`). It is the universal heap-
header + refcount primitive — every refcounted heap value in the
torajs runtime (every Str / Obj / Arr / Closure / RegExp / Date /
Promise / Map / Set / BigInt / DynObj / …) flows through these
functions. Per `rules/rust/patterns.md`: hot paths sit at µs / ns
budget; `__torajs_rc_inc` / `__torajs_rc_dec` are emitted by
`ssa_lower` at every refcounted slot assignment and slot drop, so
they are among the single hottest lines in the runtime.

## Budgets

| Path | Budget | Observed P95 (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `inc_dec_pair` (1M pairs, balanced refcount) | 20 ms | ~2 ms | ~10× | The slot-copy + drop shape; every refcounted assignment lowers to this. Per-pair ≈ 20 ns worst-case. |
| `inc_static_literal` (1M iters, FLAG_STATIC_LITERAL bypass) | 10 ms | ~1 ms | ~10× | Hot in tight loops referencing string literals; should compile to two compares + branch. |
| `header_layout_size_invariant` | functional | — | — | Asserts `size_of::<HeapHeader>() == 8` and `align_of == 8`. Not a perf gate; presence catches accidental layout drift that would shift per-type struct payload offsets and silently break `ssa_lower`'s IR const-offset arithmetic. |

Real-world hot-path cost is dominated by the null-and-flag branch
plus the single `add` on `refcount`; the criterion micro-bench
under `benches/rc.rs` measures ns/op directly (the integration test
above is the µs-aggregate variant for CI catch).

## Methodology

- Each test runs the path 1M times.
- The **median** sample is asserted under the budget, not the mean —
  median is robust to occasional GC / context-switch noise.
- Budgets are **wall-clock**, not CPU time. Tests must be runnable
  on any reasonable CI executor; we don't pin to high-resolution
  clocks or special hardware.

## When to re-measure

Update the table (and the asserts in `perf_gate.rs`) when any of
these fire:

- Algorithm change (e.g. switch to atomic refcount when threading
  lands, or to a fat-deferred / generational scheme)
- Layout change (HeapHeader fields reordered or resized — these are
  ABI-breaking; coordinate with `runtime_*.c` and the `ssa_lower`
  IR-side const offsets in the same commit)
- Code-gen change (rustc / LLVM upgrade)
- A criterion bench median in `benches/rc.rs` moves by > 30% in
  either direction — that's a real signal worth chasing, not noise.

## Vision alignment

- `inc_dec_pair` regressions feed directly into the workspace-level
  `bench-harness` geomean — every refcounted bench case (Promise /
  Closure / Array / String) exercises this path. Holding the budget
  here is necessary (but not sufficient) for holding the workspace
  geomean ≥ 4.41× vs bun-aot.
- The crate is `no_std` + zero runtime deps; only `criterion` (dev-
  shared workspace dep, approved 2026-05-22) ships in CI binaries.
  Matches vision item #4 (0 deps).
