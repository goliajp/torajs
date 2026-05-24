# torajs-dynobj performance budgets

`torajs-dynobj` sits on the **hot path** for any TS code that uses
object literal property bags. Per-op latency in low-ns territory.

| Path | Hot/Cold | Notes |
| --- | --- | --- |
| `set` | **Hot** | Per `obj.x = v`. |
| `get` | **Hot** | Per `obj.x` read. |
| `has` | Warm | Per `"x" in obj`. |
| `delete` | Warm | Per `delete obj.x`. |
| `iter` | Warm | Per `for (k in obj)` / Object.keys / values. |

## Budgets

| Path | Budget | Notes |
| --- | ---: | --- |
| Hash a key Str | < 20 ns / 8 bytes | FNV-1a on raw bytes. |
| `get` hit | < 50 ns | Hash + linear probe ≤ 3 slots + byte-equal check. |
| `set` (no resize) | < 80 ns | Hash + probe + tag/value store. |
| `set` (with resize) | ~µs amortized | Doubling resize + rehash. Amortized O(1). |
| `delete` (no compact) | < 100 ns | Tombstone set. |
| `iter` | < 10 ns / entry | Linear walk over entries[]. |

## Memory budget

- DynObj heap block: header (8 B) + len (8 B) + cap (8 B) + slots
  (cap × 8 B index entries) + entries (cap × 24 B {tag, value, key_ptr}).
- Load-factor 0.7 target → ~1.4× memory overhead vs theoretical minimum.

## What's NOT budgeted

- **String-hash collisions**: FNV-1a is non-cryptographic; adversarial
  key patterns can degrade. Not in torajs scope today.
- **Concurrent access**: single-threaded.
- **Insertion-ordered iteration** (per ES spec): not yet guaranteed.
  See README for the deferred-work item.
