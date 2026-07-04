//! Array element-kind field (Tag::Arr only) — `HeapHeader.flags`
//! bits 10-12.
//!
//! Any-dynamic-access RFC (20260704) — a typed `Array<T>`'s element
//! representation (raw i64 / raw f64 / raw bool / heap pointer) lives
//! only in SSA static types; once the array crosses into the `any`
//! world (`anyv_box_from_pair(ANY_HEAP, arr)`), every runtime consumer
//! (print, index-get, drop walker) is blind. This 3-bit field makes
//! the array self-describing — the V8 ElementsKind shape
//! (PACKED_SMI / PACKED_DOUBLE / PACKED elements, same taxonomy).
//!
//! Written by `__torajs_arr_mark_kind` (torajs-arr) at the ssa-lower
//! typed-arr→Any boxing boundary — NOT at alloc (alloc stays
//! kind-blind; arrays that never cross into `any` never pay the
//! write). [`ARR_KIND_UNSET`] (0) therefore means "never crossed" and
//! consumers fall back to their pre-RFC behaviour.
//!
//! `Array<Any>` ([`crate::FLAG_ARR_ANY`]) is already self-describing
//! via its NaN-box slots — consumers check FLAG_ARR_ANY first; this
//! field stays UNSET for those blocks.
//!
//! Bits 10-12 are Tag::Arr-private (disjoint-by-tag reuse, same
//! pattern as bit 6 = DynObj NULL_PROTO / bit 7 = Obj FLAG_ERROR).
//! Re-exported at the crate root (same shape as [`crate::color`]).

/// Shift of the 3-bit element-kind field inside `HeapHeader.flags`.
pub const ARR_ELEM_KIND_SHIFT: u16 = 10;
/// Mask of the 3-bit element-kind field.
pub const ARR_ELEM_KIND_MASK: u16 = 0b111 << ARR_ELEM_KIND_SHIFT;
/// Element kind not (yet) recorded — array never crossed into `any`.
pub const ARR_KIND_UNSET: u16 = 0;
/// Raw `i64` scalar slots (TS `number` int-repr elements).
pub const ARR_KIND_I64: u16 = 1;
/// Raw `f64` scalar slots (TS `number` float-repr elements).
pub const ARR_KIND_F64: u16 = 2;
/// Raw `0`/`1` i64 slots (TS `boolean` elements).
pub const ARR_KIND_BOOL: u16 = 3;
/// Heap-pointer slots — Str / Substr / nested Arr / Obj / Closure /
/// any other refcounted cell. Consumers treat each slot as a cell
/// pointer and dispatch on its heap header tag.
pub const ARR_KIND_HEAP: u16 = 4;

impl crate::HeapHeader {
    /// Read the 3-bit element-kind field (bits 10-12) — one of the
    /// `ARR_KIND_*` constants. Only meaningful on `Tag::Arr` headers
    /// with [`crate::FLAG_ARR_ANY`] clear; [`ARR_KIND_UNSET`] means
    /// the array never crossed the typed→Any boundary.
    #[inline]
    pub fn arr_elem_kind(&self) -> u16 {
        (self.flags & ARR_ELEM_KIND_MASK) >> ARR_ELEM_KIND_SHIFT
    }
}
