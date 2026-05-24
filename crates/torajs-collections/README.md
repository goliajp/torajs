# torajs-collections

[![Crates.io](https://img.shields.io/crates/v/torajs-collections?style=flat-square&logo=rust)](https://crates.io/crates/torajs-collections)
[![docs.rs](https://img.shields.io/docsrs/torajs-collections?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-collections)
[![License](https://img.shields.io/crates/l/torajs-collections?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-collections?style=flat-square)](https://crates.io/crates/torajs-collections)

`Map<K,V>` + `Set<K>` + `MapIter` for the [torajs] AOT TypeScript
runtime — strong-ref collections. Robin-hood hash table with separate
`slots[]` + `entries[]` arrays per the ECMAScript spec's
insertion-ordered iteration contract. 0 Cargo deps.

Extracted from `runtime_map.c` (~970 LOC of C) as **P4.3** sub-step
sequence (commits `fbb052b` through `ab95e60`, 2026-05-24). Weak
family (WeakMap / WeakSet / WeakRef) lives in `torajs-weak` —
strong vs weak ref semantics warrant a clean crate split.

## Why two arrays

JS `Map` mandates insertion-ordered iteration. Robin-hood hashing on
its own gives O(1) expected lookup but **loses insertion order on
resize / probe**. The fix is two parallel arrays:

- `entries[]` — insertion-ordered records `{key, value, hash}`,
  append-only on insertion. Order survives resizes.
- `slots[]` — compact hash → index-into-entries[] for lookup.
  Resize rebuilds slots[]; entries[] order stays.

## Key equality

`SameValueZero` per ES §7.2.10 — like `===` except `NaN === NaN` and
`+0 === -0`. Implemented for all 5 type-tag pairs (I64, F64-bits,
Str, Heap, Bool / Null / Undefined).

## Modules (11 files, ~1.5 KLOC)

| Module | Purpose |
| --- | --- |
| `lib.rs` | Re-exports + extern boundary |
| `layout.rs` | Heap layouts (Map + Set + MapIter) |
| `hash.rs` | FNV-1a over typed key + sameValueZero comparison |
| `alloc.rs` | Map/Set heap construction |
| `set.rs` | Map.set + Set.add |
| `get.rs` | Map.get / has / Set.has |
| `delete.rs` | Map.delete + Set.delete + tombstone management |
| `iter.rs` | MapIter create / step / drop |
| `for_each.rs` | Map.forEach + Set.forEach |
| `clear.rs` | Map.clear + Set.clear |
| `drop.rs` | Per-entry value drop dispatch |

## License

Dual-licensed: Apache-2.0 / MIT — see [LICENSE-APACHE](LICENSE-APACHE)
+ [LICENSE-MIT](LICENSE-MIT).

[torajs]: https://torajs.com
