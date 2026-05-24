# torajs-microtask performance budgets

`torajs-microtask` runs every Promise reaction + every `queueMicrotask`
in user code. For Promise-heavy workloads (promise-chain-1k, promise-
then-100k) the per-task enqueue + dispatch cost shows up directly in
bench numbers. Latency budget per op is in low-ns territory.

## Path taxonomy

| Path | Hot/Cold | Notes |
| --- | --- | --- |
| `enqueue` | **Hot** | Per Promise reaction + per `queueMicrotask` + per `.then` setup. |
| `run_until_idle` | **Hot** | Per drain — usually paired 1:1 with the top-level task that scheduled them. |
| `pending_count` | Warm | Used by Promise resolve-or-reject double-call check; per Promise lifecycle. |

## Budgets

| Path | Budget | Observed (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `microtask_enqueue_drain_burst_8-10k` | ≤ 5 ms | ~2 ms | 2.5× | 10k cycles × (8 enqueues + drain) = 80k task ops. ~25 ns / task amortized. |
| `microtask_enqueue_drain_burst_64-1k` | ≤ 2 ms | ~1 ms | 2× | 1k cycles × (64 enqueues + drain) = 64k task ops. ~15 ns / task — amortizes the compaction cost across larger bursts. |

## Memory budget

- Static globals: 1 `Vec<TaskRecord>` (24 B header + capacity-allocated
  payload). Steady-state capacity for Promise-heavy bench corpus is
  ~256 entries × 16 B = 4 KB resident.
- Compaction triggers at `head > cap/2`; bounded by `O(queue_max_depth)`.

## Integrated bench corpus impact

Per `bench/results/2026-05-24-mini-9b7740c.json` (Phase 1 closed):

- `promise-await-100k` torajs: 1.94 ms (~5k microtasks/sec measured)
- `promise-chain-1k` torajs: 1.39 ms (~1k microtask chain depth)
- `promise-then-100k` torajs: 4.38 ms (~100k microtask dispatches)
- `promise-all-1k` torajs: 1.40 ms (~10k microtask sync points)

These numbers include Promise state-machine cost, not just the
microtask queue. The queue's contribution is the per-task ~15-25 ns
overhead measured by the benches above.

## What's NOT budgeted

- **Promise state-machine latency**: belongs to `torajs-promise`'s budget.
- **`fn_ptr` invocation cost**: caller-supplied fn body cost is out of
  our scope; we only measure the dispatch overhead.
- **Multi-threaded queue**: torajs is single-threaded today. Per-worker
  queues post-v1.0 would change the budget shape (no contention, but
  per-worker memory cost).
