# torajs-collections performance budgets

| Path | Hot/Cold | Notes |
| --- | --- | --- |
| `Map.set / get / has / delete` | **Hot** | Per Map op. |
| `Set.add / has / delete` | **Hot** | Per Set op. |
| `MapIter step` | Warm | Per for-of iteration step. |
| `Map.forEach` | Warm | Bulk iteration. |

| Path | Budget |
| --- | ---: |
| `Map.set` (no resize) | < 100 ns |
| `Map.get` hit | < 60 ns |
| `Map.delete` (no compact) | < 100 ns |
| `MapIter step` | < 30 ns |

## Memory budget

- Map heap block: header + len + cap + 2 arrays.
- `slots[]`: cap × 4 B (u32 index).
- `entries[]`: cap × 32 B (key tag + key value + value tag + value value).
- Total: ~36 × cap bytes. Robin-hood load factor 0.75.

## What's NOT budgeted

- Adversarial key collision rates (FNV-1a non-cryptographic).
- Concurrent mutation.
- Insertion-ordered iter is guaranteed; per-op ordering invariants
  exercised by conformance gate fixtures.
