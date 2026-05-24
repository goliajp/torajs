# torajs-capture-box performance budgets

`torajs-capture-box` sits on the **closure construction hot path**:
every escape-captured `let` slot promoted to the heap goes through
one of `alloc` / `inc` / `drop` per closure-env construct + scope-end
edge. For torajs's closure-heavy benchmarks (closure-counter ~10k
ops, closure-pipeline-1m ~1M ops) this is a per-op micro-budget.

Budgets here are documentary (no perf-gate tests); `benches/capture_box.rs`
reports the actual numbers via criterion.

## Path taxonomy

`torajs-capture-box` is **Layer-1** in the architecture-rewrite stack —
sits below `torajs-promise` / `torajs-arr` / Layer-2+ and only above
Rust's `std::alloc`. The 3-fn extern surface (`alloc` / `inc` /
`drop`) is the minimum to support closure-captured `Copy`-typed
locals; no dispatch / drop callbacks / tag inspection.

## Budgets

| Path | Budget | Observed (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `alloc-inc-drop-cycle-100k` | ≤ 5 ms | ~2 ms | ~2.5× | One captured-let lifetime per iter. `alloc` = `std::alloc::alloc(16, 8)` + 2 stores; `inc` = 1 load + add + store; `drop` = 1 load + cmp + dec + store + maybe `dealloc`. |
| `alloc-drop-no-inc-100k` | ≤ 5 ms | ~2 ms | ~2.5× | Heap-promoted-but-not-captured edge case (rc starts at 0, drop fires the at-zero-observation free arm). |

## Allocation count

`alloc-inc-drop-cycle` allocates + frees one 16-byte block per
captured-let lifetime. There is no pooling — each box is a fresh
`std::alloc::alloc` followed by an `std::alloc::dealloc`. Future
polish could add a LIFO pool (cf. `torajs-pool`) but the workspace's
current closure-heavy benches haven't profiled this as a bottleneck;
the alloc itself is ~30 ns on aarch64 macOS, which is already at
the edge of bench measurement noise.

## Layout invariant (correctness budget)

The value-slot pointer is `base + 8`. `__torajs_capture_box_inc` /
`_drop` reach back to `slot - 8` for the refcount. The 16-byte block
is allocated 8-byte aligned (via `Layout::from_size_align_unchecked(16, 8)`)
so `*mut u64` at `base + 0` and `*mut i64` at `base + 8` are both
naturally aligned.

If a future refactor changes the box layout (e.g. adds a type tag),
the alignment + pointer-offset invariants must be preserved or the
SSA-lower codegen patterns (`Load i64 at slot+0` / `Store i64 at
slot+0`) must change in lockstep. `tests/lifecycle.rs::value_slot_8_aligned`
catches violations to the alignment half of this.

## What's NOT budgeted

- **Concurrent inc/dec**: torajs is single-threaded today; `*rc += 1`
  is plain. Atomic refcount would cost ~5-10 ns extra on aarch64
  (LL/SC pair) but isn't paid until threading lands.
- **Smaller (8-byte) box for bool**: a bool-only captured let still
  uses the 16-byte box (8 bytes wasted to alignment + the i64 widening
  in the value slot). A specialized small-box variant could save
  alloc bytes but doubles the surface; not done.
- **Reuse via a per-thread LIFO pool**: see "Allocation count" above.
  Defer until profiling shows it matters.
