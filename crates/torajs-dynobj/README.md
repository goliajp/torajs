# torajs-dynobj

[![Crates.io](https://img.shields.io/crates/v/torajs-dynobj?style=flat-square&logo=rust)](https://crates.io/crates/torajs-dynobj)
[![docs.rs](https://img.shields.io/docsrs/torajs-dynobj?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-dynobj)
[![License](https://img.shields.io/crates/l/torajs-dynobj?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-dynobj?style=flat-square)](https://crates.io/crates/torajs-dynobj)

Dynamic-property-bag substrate for the [torajs] AOT TypeScript runtime
— the open-addressing hashmap behind `obj.x = v`, `arr.x = v`,
`fn.x = v` patterns. 0 Cargo deps; Swift-`Dictionary`-/ CPython-`dict`-shape
algorithm, fully self-implemented per the torajs 自研 pillar.

Extracted from `runtime_str.c` as P4.2 (closed `17397a5`, 2026-05-23).
11 source modules covering insertion / lookup / removal / iteration /
property-descriptor attributes + drop integration.

## Algorithm choices

| Op | Algorithm | Source |
| --- | --- | --- |
| Hash | FNV-1a 64-bit on key bytes | Standard |
| Probing | Linear (Swift-Dictionary shape) | Standard |
| Resize | Doubling at load-factor > 0.7 | Standard |
| Eq | Byte-equal on key Str (Same-Value comparison) | ECMAScript §7.2.10 |

## Module layout

| Module | Purpose |
| --- | --- |
| `lib.rs` | Re-exports + top-level helper externs |
| `layout.rs` | Heap layout (header + slot[] + bucket[] arrays) |
| `alloc.rs` | DynObj heap-block construction |
| `set.rs` | `obj.x = v` insertion + resize logic |
| `get.rs` | `obj.x` lookup |
| `delete.rs` | `delete obj.x` removal + tombstone management |
| `has.rs` | `"x" in obj` containment check |
| `define.rs` | `Object.defineProperty` (writable / enumerable / configurable flags) |
| `attrs.rs` | PropertyDescriptor attribute bit packing |
| `iter.rs` | Insertion-ordered key iteration |
| `drop.rs` | Per-tag entry drop + walk_blk integration |

## What this crate is NOT

- **Not a Map<K,V>**: keys here are always strings. For arbitrary-key
  Maps, see `torajs-collections`.
- **Not the universal heap header**: see `torajs-rc` for refcount + tag.
- **Not insertion-ordered for iteration** in the strict spec sense:
  resize triggers reindex; iteration order is "any consistent order"
  rather than strict insertion order. ECMAScript spec **requires**
  insertion order for ES2015+; if your TS code depends on this,
  use a Map.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
