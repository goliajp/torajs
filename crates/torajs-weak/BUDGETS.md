# torajs-weak performance budgets

| Path | Hot/Cold | Notes |
| --- | --- | --- |
| `WeakRef.create / deref / drop` | Warm | Per WeakRef lifecycle. |
| `WeakMap.set / get / has / delete` | Warm | Per WeakMap op. |
| `WeakSet.add / has / delete` | Warm | Per WeakSet op. |
| `target_died` walker | Cold | Per rc-dec on observed target. |

| Path | Budget |
| --- | ---: |
| WeakRef.create / deref | < 100 ns |
| WeakRef.drop | < 200 ns (registry unsubscribe) |
| WeakMap.set / get | < 200 ns (hash + probe + observer-link) |
| target_died walker | O(observers) — typically 1-2 |

## What's NOT budgeted

- FinalizationRegistry path (not yet shipped).
- Concurrent weak-target eviction.
- Cross-realm weak refs.
