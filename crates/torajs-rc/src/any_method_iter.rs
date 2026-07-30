//! Iterator-helper method ids (RFC 20260730-iterator-global 刀 2b)
//! — sibling of `any_method.rs` (that file sits at the 500-line
//! cap; the iterator family grows here, same append-only lockstep
//! with `any_method_intern.rs`).

/// `take` — §27.1.4.9 lazy limit helper.
pub const ANY_METHOD_TAKE: i64 = 171;

/// `drop` — §27.1.4.3 lazy skip helper.
pub const ANY_METHOD_DROP: i64 = 172;

/// `toArray` — §27.1.4.10 eager collector.
pub const ANY_METHOD_TO_ARRAY: i64 = 173;
