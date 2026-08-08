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

/// `%Iterator.prototype%[Symbol.iterator]` — §27.1.2.1 return-this
/// (own id, never interns back; the spec function name has
/// brackets). Iterator cells (MapIter / ArrIter / IterHelper) reify
/// their `@@iterator` read to this id (刀 4 长尾).
pub const ANY_METHOD_ITER_SELF: i64 = 174;

/// `deref` — §26.1.4.2 `WeakRef.prototype.deref`. The one method on
/// the third weak family, which joined the any lane in rotation 314
/// (its typed receiver had a lowering all along; a WeakRef reached
/// through `any` had no arm to land in).
pub const ANY_METHOD_DEREF: i64 = 175;

/// `%Iterator.prototype%[Symbol.dispose]` — §27.1.4.1 (Explicit
/// Resource Management): GetMethod(this, "return"), call it when
/// present, answer undefined. Own id like [`ANY_METHOD_ITER_SELF`]
/// (the spec function name has brackets, never interns back);
/// iterator cells reify their `@@dispose` read to this id (RFC
/// 20260809 B6).
pub const ANY_METHOD_ITER_DISPOSE: i64 = 176;
