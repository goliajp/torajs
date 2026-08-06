//! Which collection a constructor's iterable initializer is filling —
//! the selector `ssa_lower` emits and
//! `__torajs_collection_init_from_iterable` switches on. Shared here
//! because both sides must agree on the numbering and neither owns
//! the other.
//!
//! §24.1.1.1 (Map) / §24.2.2.1 (Set) / §24.3.1.1 (WeakMap) /
//! §24.4.1.1 (WeakSet) are one algorithm; the pairs differ only in
//! whether an item is an entry (`set(k, v)`) or a value (`add(v)`).

/// `new Map(iterable)` — entries.
pub const COLLECTION_MAP: i64 = 0;
/// `new Set(iterable)` — values.
pub const COLLECTION_SET: i64 = 1;
/// `new WeakMap(iterable)` — entries, weak keys.
pub const COLLECTION_WEAKMAP: i64 = 2;
/// `new WeakSet(iterable)` — values, weak keys.
pub const COLLECTION_WEAKSET: i64 = 3;
