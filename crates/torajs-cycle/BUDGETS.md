# torajs-cycle performance budgets

The collector runs **rarely** (auto-trigger threshold + manual gc() +
main-exit drain). Per-collection cost matters; per-rc_dec
`__torajs_cycle_buffer_push` overhead is hot path.

## Path taxonomy

| Path | Hot/Cold | Notes |
| --- | --- | --- |
| `__torajs_cycle_buffer_push` | **Hot** | Per rc_dec on a class instance / array. |
| `__torajs_cycle_buffer_unpush` | Warm | Per object freed by direct rc-dec (so it's not collected later). |
| `collect()` mark / scan / collect | Cold | Triggered rarely. |
| `gc()` user-callable | Cold | Per `gc()` user call. |

## Per-op budgets

| Path | Budget | Notes |
| --- | ---: | --- |
| `buffer_push` | < 20 ns | One AtomicUsize increment + slot store. |
| `buffer_unpush` | < 50 ns | Linear scan + swap-remove. (Rare; only fires when direct rc-dec drains the cycle's last ref before collect could.) |
| `collect()` setup | < 1 µs | Buffer drain + state machine init. |
| Per node marked | < 50 ns | Recursive walk + flag bit set. |
| Per node scanned | < 50 ns | Refcount-minus-internal arithmetic + flag bit set. |
| Per node freed | < 200 ns | Refcount drop + per-tag _drop dispatch + free. |

## Memory budget

- Global trial buffer: `Vec<*mut c_void>` with `AUTO_COLLECT_THRESHOLD`
  = 8192 capacity (peak; doubled on overflow). ~64 KB at threshold.
- No additional per-object overhead: cycle colors live in the
  universal heap header's `flags` field (2 spare bits).

## What's NOT budgeted

- **Mark/scan recursion depth**: bounded by class-instance graph
  shape — pathological deep nesting could blow the stack. Worst-
  case `unwrap_unchecked` would need refactor to iterative.
- **GC pauses for large heaps**: linear in candidate-count. The
  threshold trigger keeps individual pauses small.
- **Concurrent collection**: single-threaded; the collector runs
  to completion synchronously.
