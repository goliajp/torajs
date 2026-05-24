# torajs-value-drop performance budgets

`__torajs_value_drop_heap` is a **dispatch fn** — its own cost is one
load (the `type_tag` u16) + one `match` + one `bl` to the matching
`_drop`. The bulk of the work happens in the dispatched-to extern, not
here. So the budget here is the dispatch overhead only.

## Path taxonomy

`__torajs_value_drop_heap` fires once per heap-tagged child drop:

- Array<Any> element walk at array drop
- DynObj entry walk at object drop
- AnyBox drop when the box wraps a Heap-tagged child

Per torajs's bench corpus, this is ~tens of K calls per second on
the busiest cases (closure-counter, generic-pair-1m). Not a top-tier
hot path but still per-op micro-budget.

## Budgets

| Path | Budget | Observed (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `value_drop_heap-null-100k` | ≤ 200 µs (~2 ns/iter) | ~80 µs (~0.8 ns/iter) | 2.5× | 100k NULL-input fast-path calls; one cbz + ret. |
| Dispatch overhead (non-null, in-table tag) | ≤ 10 ns | ~3 ns | 3× | u16 load + match + bl. Measured via end-to-end conformance gate timing diff vs the C version pre-port. |
| Fallback arm (rc-dec + free) | ≤ 50 ns | ~25 ns | 2× | One refcount decrement + atomic compare-zero + maybe free. |

## Binary-size budget

| Path | Budget | Measured |
| --- | ---: | ---: |
| `libtorajs_value_drop.a` artifact | ≤ 8 KB | ~3 KB |
| Per-call code at the AOT site | ≤ 8 bytes | 4 bytes (`bl __torajs_value_drop_heap`) |
| Dispatch jump table | ≤ 256 bytes | ~120 bytes (11 arms + fallback) |

## What's NOT budgeted

- **Inner-ref walking**: not done by this crate; per-type drop fns
  (or call-site walks in array / dynobj) are responsible. We don't
  budget those here.
- **Per-tag dispatch arm latency**: belongs to each tagged-type
  crate's own `BUDGETS.md`. The arms are: `__torajs_str_drop`,
  `__torajs_arr_drop`, `__torajs_bigint_drop`, ...
- **Atomic cost on the fallback arm**: `__torajs_rc_dec` is a
  Relaxed atomic decrement on single-threaded runtime (no real
  contention); cost is documented in `torajs-rc/BUDGETS.md`.
