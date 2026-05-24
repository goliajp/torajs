# torajs-weak

[![Crates.io](https://img.shields.io/crates/v/torajs-weak?style=flat-square&logo=rust)](https://crates.io/crates/torajs-weak)
[![docs.rs](https://img.shields.io/docsrs/torajs-weak?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-weak)
[![License](https://img.shields.io/crates/l/torajs-weak?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-weak?style=flat-square)](https://crates.io/crates/torajs-weak)

Weak-reference family (`WeakRef` + `WeakMap` + `WeakSet`) for the
[torajs] AOT TypeScript runtime. Shared process-global target →
observer registry. 0 Cargo deps.

Extracted from `runtime_weakref.c` / `runtime_weakmap.c` /
`runtime_weakset.c` (~620 LOC of C combined) as **P4.3'** sub-step
sequence (commits `c2a456e` / `5be15e9` / `555962d` / `56ac46d`,
2026-05-24).

## Design

Single global registry maps each weakly-observed target object to
the list of observers (WeakRef instances + WeakMap entries +
WeakSet entries). When a heap target's strong refcount drops to
zero, `torajs-rc`'s dec hook calls `__torajs_weak_target_died(p)`,
which walks the registry entry for `p` and clears each observer.

WeakRef clears its target slot to NULL; WeakMap evicts the entry
keyed by `p`; WeakSet evicts the membership entry.

## Modules (8 files, ~1.3 KLOC)

| Module | Purpose |
| --- | --- |
| `lib.rs` | Top-level re-exports + shared registry global |
| `layout.rs` | Heap layouts for WeakRef / WeakMap / WeakSet |
| `registry.rs` | Target → observers map + walker hook |
| `weakref.rs` | WeakRef create / deref / drop |
| `weakmap.rs` | WeakMap surface |
| `weakset.rs` | WeakSet surface |
| `weakmap_iter.rs` | Internal iter helper used by drop walks |
| `weakset_iter.rs` | Internal iter helper used by drop walks |

## Cross-tier integration

The registry is **opt-in**: only objects that have been weakly
observed end up in the registry. The walker hook is called by
`torajs-rc::__torajs_rc_dec` only when the target's `flags` bit
indicates it has observers — zero overhead for objects that are
never weakly observed.

## What's NOT in scope

- **FinalizationRegistry**: per ES2021 spec — not yet shipped.
- **Concurrent weak-target collection**: single-threaded.
- **Cross-realm weak refs**: realm separation not modeled today.

## License

Dual-licensed: Apache-2.0 / MIT — see [LICENSE-APACHE](LICENSE-APACHE)
+ [LICENSE-MIT](LICENSE-MIT).

[torajs]: https://torajs.com
