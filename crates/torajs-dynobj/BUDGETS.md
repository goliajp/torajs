# torajs-dynobj performance budgets

`torajs-dynobj` sits on the **hot path** for any TS code that uses
object literal property bags. Per-op latency in low-ns territory.

| Path | Hot/Cold | Notes |
| --- | --- | --- |
| `set` | **Hot** | Per `obj.x = v`; `set_own` is the one-probe write into an existing own entry. |
| `get` | **Hot** | Per `obj.x` read; `get_own` answers tag and value from one probe. |
| `has` | Warm | Per `"x" in obj`. |
| `delete` | Warm | Per `delete obj.x`. |
| `iter` | Warm | Per `for (k in obj)` / Object.keys / values. |

## Budgets

These are targets. None of them is measured: the crate has no
`tests/perf_gate.rs`, and `benches/dynobj.rs` is a placeholder (a
dynobj bench needs live Str and refcount cells). End-to-end timing
comes from the bench corpus (`dict-count-optional-read-1m`,
`objlit-optional-field-1m`, `csv-trim-100k`).

| Path | Budget | Notes |
| --- | ---: | --- |
| Hash a key Str | < 20 ns / 8 bytes | FNV-1a over the payload bytes (Latin-1 or UTF-16); a Symbol key hashes its cell address. |
| `get` hit | < 50 ns | Hash + linear probe of the u32 index + key compare + NaN-box decode. |
| `set` (no resize) | < 80 ns | Hash + probe + NaN-box encode into the entry. |
| `set` (with resize) | ~µs amortized | Fresh store, holes compacted out, index rebuilt; grows to the smallest power of 2 above `count * 3` when genuinely full. Amortized O(1). |
| `delete` (no compact) | < 100 ns | Index slot becomes a tombstone, entry becomes a hole. |
| `iter` | < 10 ns / entry | Linear walk over the dense entry array, holes skipped. |

## Memory budget

- Header cell: 32 B, address-stable for the object's lifetime
  (heap header 8 B + count / cap / entries_len / entries_cap 4 × 4 B +
  store pointer 8 B).
- Store block (swapped on resize): `cap × 4 B` hash index (u32 entry
  indices) + `entries_cap × 16 B` dense entries
  (`{ key_ptr_tagged, value_anyv }`, the W/E/C flags in the key
  pointer's low bits), with `entries_cap = cap × 7/8`. Initial
  `cap` is 8.
- Not measured against a theoretical minimum.

## What's NOT budgeted

- **String-hash collisions**: FNV-1a is non-cryptographic; adversarial
  key patterns can degrade. Not in torajs scope today.
- **Concurrent access**: single-threaded.
