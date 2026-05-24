# torajs-promise performance budgets

Per-op latency in 10-100 ns range. Pool hit/miss is the biggest
variable. Refer to bench corpus baseline numbers in
`bench/results/2026-05-24-mini-9b7740c.json` for end-to-end timing.

| Path | Hot/Cold |
| --- | --- |
| `alloc_pending` | Hot |
| `resolve` / `reject` | Hot |
| `.then` callback attach | Hot |
| Callback chain dispatch | Hot |
| Pool acquire / release | Hot |

| Path | Budget |
| --- | ---: |
| Promise alloc (pool hit) | < 30 ns |
| Promise alloc (pool miss) | < 100 ns (std::alloc heap) |
| resolve / reject + cb dispatch | < 200 ns |
| .then attach to pending | < 100 ns |

## Memory budget

- Promise heap block: 32 B (header + state + value + cb_head).
- Pool: 32 slots × 32 B = 1 KB peak.
- Cb-chain entries: 24 B each, allocated lazily.

## What's NOT budgeted

- User-supplied callback fn body cost.
- AggregateError construction (rare, large path).
- Cross-realm Promise.race (realm separation not modeled).
