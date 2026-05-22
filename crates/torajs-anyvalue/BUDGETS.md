# torajs-anyvalue performance budgets

Latency budgets enforced by `tests/perf_gate.rs`. Each budget is
set with ~10× headroom over observed P95 on a dev machine so CI
fails on order-of-magnitude regressions, not micro-noise.

Run `cargo test -p torajs-anyvalue --test perf_gate` to check.

## Path taxonomy

`torajs-anyvalue` sits at Layer 1 of the torajs runtime crate
stack (see `docs/architecture-rewrite.md`). Every `Type::Any`-
typed slot in user code flows through an `AnyBox`: every
`Array<Any>` element, every dynamic-property bag value, every
function `: any` parameter / return value boxes on the way in or
out. Per `rules/rust/patterns.md`: hot paths sit at µs / ns
budget.

## Budgets

| Path | Budget | Observed P95 (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `method_alloc_drop_pair` (100k iters, I64 tag) | 50 ms | ~5 ms | ~10× | The `AnyBox::alloc + drop_owned` shape; every transient Any value (function call site, generic monomorphization escape) hits this. |
| `ffi_alloc_drop_pair` | 50 ms | ~5 ms | ~10× | Same shape via the FFI shim. Verifies the shim adds no measurable overhead under fat LTO vs the method API. |
| `anybox_layout_size_invariant` | functional | — | — | Asserts `size_of::<AnyBox>() == 24` and `align_of == 8`. Layout drift would shift every const-offset read ssa_lower emits at dynobj / Array<Any> sites. |

## Methodology

- Each test runs the path 100k times.
- Median sample asserted under the budget (robust to occasional
  GC / context-switch noise).
- Budgets are wall-clock, not CPU time.

## When to re-measure

- Algorithm change (e.g. AnyBox pooled to LIFO free-list, P-PERF
  follow-up)
- Layout change (struct grew / fields reordered)
- Code-gen change (rustc / LLVM upgrade)
- Criterion bench median in `benches/anyvalue.rs` moves > 30% in
  either direction — chase it as a real signal.

## Vision alignment

`box_unbox_i64` regressions feed directly into the workspace-
level `bench-harness` geomean: every Promise / Closure / generic
case that touches an Any-typed value materializes one or more
`AnyBox` allocations. Holding the budget here is necessary (but
not sufficient) for the workspace geomean ≥ 4.41× vs bun-aot
invariant.

Vision item #4 (0 deps): `cargo tree -p torajs-anyvalue` shows
only the in-workspace `torajs-rc` dependency. `criterion` is
workspace-dev-shared and not present in produced binaries.
