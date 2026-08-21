//! NaN-box encode + decode-tag/value FFI shims —
//! `__torajs_anyv_box_*` + `__torajs_anyv_unbox_tag` +
//! `__torajs_anyv_unbox_value` (Step 7b / 7d).
//!
//! Also home of the **pair-arg bridge shims** that 7d-A's
//! ssa_lower atomic switch swaps to from the legacy
//! `*AnyBox`-returning `any_arith` / `any_add` /
//! `any_strict_eq` declares — these take the `(tag, value)`
//! pairs ssa_lower already produces (post-`unbox_tag/value`)
//! and return an immediate AnyValue (no AnyBox alloc).
//!
//! Split out of [`crate::nanbox_ffi`] to keep that file under
//! the 500-line hard limit. The encode/unbox shims are the
//! migration entry points ssa_lower calls into; the rest
//! (rc_inc/dec, to_number/to_str/to_bool, strict_eq,
//! compare/arith/add) stays in `nanbox_ffi.rs`.

use std::ffi::c_void;

use torajs_rc::AnySlotTag;

use crate::arith::{any_add, any_arith};
use crate::arith_bitwise::{any_bitnot, any_bitwise};
use crate::coerce::any_to_str;
use crate::compare::any_compare;
use crate::nanbox::{
    AnyValue, VALUE_FALSE, VALUE_NULL, VALUE_TRUE, VALUE_UNDEFINED, as_bool, as_double, as_int32,
    box_bool, box_double, box_int32, box_void_ptr, is_bool, is_cell, is_double, is_int32, is_null,
    is_short_str, is_undefined,
};
use crate::nanbox_ffi::__torajs_anyv_strict_eq;
use crate::nanbox_ffi_materialize::{drop_materialized_str, materialize_short_str};
use crate::payload_rc_inc;

// ============================================================
// Encode shims — let ssa_lower emit one-line calls instead of
// hand-rolling the bit-twiddle in IR. Cheap (every fn is one or
// two integer ops) but they make the migration in 7c-7f a pure
// callsite swap.
// ============================================================

/// Encode a 32-bit signed integer as an [`AnyValue`].
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_box_i32(x: i32) -> AnyValue {
    box_int32(x)
}

/// Encode an `f64` as an [`AnyValue`].
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_box_f64(x: f64) -> AnyValue {
    box_double(x)
}

/// Encode a `bool` (passed as `i64` so ssa_lower can pass its
/// zext-i64 representation directly; any nonzero value → true).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_box_bool(b: i64) -> AnyValue {
    box_bool(b != 0)
}

/// Encode a heap pointer as an [`AnyValue`]. Caller transfers
/// ownership of the rc — no `rc_inc` is performed here.
///
/// # Safety
///
/// `p` is null or a valid `*mut HeapHeader` whose top 16 bits are
/// zero (aarch64 user-VA, 48 bits).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_box_pointer(p: *mut c_void) -> AnyValue {
    if p.is_null() {
        return VALUE_NULL;
    }
    box_void_ptr(p)
}

unsafe extern "C" {
    /// RFC 20260707 chunk 3 — the immortal `undefined` sentinel Str
    /// cell (torajs-str undef_sentinel.rs). A Str slot crossing into
    /// the Any world decodes against its address.
    fn __torajs_str_undef() -> *mut u8;
    /// Rotation 185 — the immortal `undefined` sentinel Substr view
    /// (string index OOB read). A Substr slot crossing into the Any
    /// world decodes against its address the same way.
    fn __torajs_substr_undef() -> *mut u8;
    /// Fresh owned Str with a view's text (torajs-str
    /// `substr_methods.rs`). An INLINE view boxed into the any world
    /// is materialized through it — see [`materialize_inline_view`].
    fn __torajs_substr_to_owned(v: *const u8) -> *mut c_void;
}

/// `FLAG_SUBSTR_INLINE` mirror (torajs-str `substr.rs`, header flags
/// bit 0): a view whose 32-byte cell lives in the tail of the split
/// block that produced it.
const FLAG_SUBSTR_INLINE: u16 = 1 << 0;

/// The any world cannot hold an inline view: the cell's storage is
/// the split block's and dies with it, and the box would point at
/// reclaimed memory the moment the typed-tier array went away
/// (`a.map(x => x)` through the any callback lane printed `["6","q!"]`
/// once the source array dropped — rotation 468). So a view leaving
/// the typed tier for `any` is materialized into an owned string,
/// which the box then owns outright (rc 1 = the box's stake). A
/// standalone view (FLAG_SUBSTR_VIEW only) owns its own cell and is
/// refcounted like any heap value, so it is shared as before; the
/// sentinels never reach here.
#[inline]
unsafe fn materialize_inline_view(p: *mut c_void) -> *mut c_void {
    let flags = unsafe { (*(p as *const torajs_rc::HeapHeader)).flags };
    if flags & FLAG_SUBSTR_INLINE != 0 {
        unsafe { __torajs_substr_to_owned(p as *const u8) }
    } else {
        p
    }
}

/// Encode a Str-typed slot value as an [`AnyValue`] (RFC 20260707
/// chunk 3). A Str slot carries three shapes: NULL (JS null), the
/// undefined sentinel cell, or a real heap Str. Rc-neutral exactly
/// like [`__torajs_anyv_box_from_pair`] tag-4, which the ssa_lower
/// `box_to_any` Str arm called before — ownership of the heap case
/// stays whatever the call site arranged.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_box_str_slot(p: *mut c_void) -> AnyValue {
    if p.is_null() {
        return VALUE_NULL;
    }
    if p == unsafe { __torajs_str_undef() } as *mut c_void {
        return VALUE_UNDEFINED;
    }
    box_void_ptr(p)
}

/// Encode a Substr-typed slot value as an [`AnyValue`] (rotation
/// 185 — Substr mirror of [`__torajs_anyv_box_str_slot`]). A Substr
/// slot carries three shapes: NULL, the Substr-shaped undefined
/// sentinel view (string index OOB read), or a real heap Substr.
/// Rc-neutral like every box-family kernel — the caller owns the
/// stake story.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_box_substr_slot(p: *mut c_void) -> AnyValue {
    if p.is_null() {
        return VALUE_NULL;
    }
    if p == unsafe { __torajs_substr_undef() } as *mut c_void {
        return VALUE_UNDEFINED;
    }
    // An inline view is copied out as an owned string the box owns;
    // the caller's stake on the view cell is a header bump the inline
    // drop path never reads (rotation 468).
    box_void_ptr(unsafe { materialize_inline_view(p) })
}

/// `(tag, …)` half of the Str-slot pair decode (RFC 20260707
/// chunk 3): 0=Null for NULL, 5=Undef for the sentinel cell,
/// 4=Heap otherwise. Pure read — the companion
/// [`__torajs_anyv_str_slot_value`] takes the stake.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_str_slot_tag(p: *mut c_void) -> i64 {
    if p.is_null() {
        AnySlotTag::Null as i64
    } else if p == unsafe { __torajs_str_undef() } as *mut c_void {
        AnySlotTag::Undef as i64
    } else {
        AnySlotTag::Heap as i64
    }
}

/// `(tag, …)` half of the Substr-slot pair decode (rotation 185 —
/// Substr mirror of [`__torajs_anyv_str_slot_tag`], identity
/// compared against the Substr-shaped sentinel view).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_substr_slot_tag(p: *mut c_void) -> i64 {
    if p.is_null() {
        AnySlotTag::Null as i64
    } else if p == unsafe { __torajs_substr_undef() } as *mut c_void {
        AnySlotTag::Undef as i64
    } else {
        AnySlotTag::Heap as i64
    }
}

/// `(…, value)` half of the Substr-slot pair decode (rotation 185 —
/// mirror of [`__torajs_anyv_str_slot_value`]): 0 for both nullish
/// shapes, the pointer + rc_inc for a heap Substr view (the slot
/// takes its +1; the sentinel's FLAG_STATIC_LITERAL makes the inc a
/// no-op path it never reaches anyway).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_substr_slot_value(p: *mut c_void) -> i64 {
    if p.is_null() || p == unsafe { __torajs_substr_undef() } as *mut c_void {
        0
    } else {
        // An inline view is copied out as an owned string — rc 1 IS
        // the slot's +1; a standalone view takes its +1 as before
        // (rotation 468).
        let q = unsafe { materialize_inline_view(p) };
        if q == p {
            payload_rc_inc(AnySlotTag::Heap as i64, p as i64);
        }
        q as i64
    }
}

/// `(…, value)` half of the Str-slot pair decode: 0 for both
/// nullish shapes (an Undef/Null pair's value MUST be 0 — strict-eq
/// compares the reconstructed box bit-for-bit), the pointer +
/// rc_inc for a heap Str (mirrors the explicit `emit_rc_inc` the
/// ssa_lower `box_to_tag_value` arm used to emit).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_str_slot_value(p: *mut c_void) -> i64 {
    if p.is_null() || p == unsafe { __torajs_str_undef() } as *mut c_void {
        0
    } else {
        payload_rc_inc(AnySlotTag::Heap as i64, p as i64);
        p as i64
    }
}

/// Encode an `i64` as an [`AnyValue`]. Values that fit in `i32`
/// become tagged Int32 immediates; values outside that range
/// promote to `f64` (lossless for ±2^53; precision loss beyond
/// matches JS semantics — Number is f64-backed).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_box_i64(x: i64) -> AnyValue {
    if let Ok(n32) = i32::try_from(x) {
        box_int32(n32)
    } else {
        box_double(x as f64)
    }
}

/// Encode a legacy `(tag, value)` pair as an [`AnyValue`].
/// Bridge for ssa_lower sites where the tag is a runtime value
/// (forward-Any: `throw e`, `consume_any`) and not known at
/// compile time. The tag follows the [`AnySlotTag`] enum
/// numbering: 0=Null, 1=Bool, 2=I64, 3=F64, 4=Heap, 5=Undef.
///
/// # Safety
///
/// For `tag == 4` (Heap), `value` must be 0 or a valid
/// `*mut HeapHeader` (the caller transfers ownership of an rc).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> AnyValue {
    match tag {
        0 => VALUE_NULL,
        1 => box_bool(value != 0),
        2 => __torajs_anyv_box_i64(value),
        3 => box_double(f64::from_bits(value as u64)),
        4 => {
            if value == 0 {
                VALUE_NULL
            } else {
                box_void_ptr(value as *mut c_void)
            }
        }
        5 => VALUE_UNDEFINED,
        _ => VALUE_NULL,
    }
}

/// S2.39 — the boxed adapter's literal-default substitution: answer
/// `v` unless it is undefined, in which case box the compile-time
/// `(tag, bits)` literal (Number / Bool / short string — ES
/// §10.2.1.3 fires a default for a missing AND an explicit-undefined
/// argument alike; the adapter's argv padding makes both arrive as
/// the undefined box). Tag 6 is private to this kernel: `bits` IS
/// the complete prebaked box (a ShortStr immediate the compiler
/// encoded), not a pair to decode. Immediates only — no ownership
/// transfer either way.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_or_default(v: AnyValue, tag: i64, bits: i64) -> AnyValue {
    if is_undefined(v) {
        if tag == 6 {
            bits as AnyValue
        } else {
            unsafe { __torajs_anyv_box_from_pair(tag, bits) }
        }
    } else {
        v
    }
}

/// Borrow-shaped cell-pointer read (chunk 712): a heap cell decodes
/// to its pointer bits, everything else — immediates INCLUDING
/// ShortStr — answers 0. The lowering's class-candidate dispatch
/// consumes this where it used to call [`__torajs_anyv_unbox_value`]
/// as if it were a borrow: that shim MATERIALIZES a ShortStr into an
/// owned heap Str the dispatch never dropped (~32B leaked per member
/// read through an any receiver) and hands an int32 immediate back
/// as raw integer "pointer" bits the class-tag load would then
/// dereference.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_cell_ptr(v: AnyValue) -> i64 {
    if is_cell(v) { v as i64 } else { 0 }
}

/// Return the `null` sentinel.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_null() -> AnyValue {
    VALUE_NULL
}

/// Return the `undefined` sentinel.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_undefined() -> AnyValue {
    VALUE_UNDEFINED
}

// ============================================================
// Decode shims — pair recovery for legacy callers
// ============================================================

/// Decode an [`AnyValue`] to its [`AnySlotTag`] enum index
/// (0=Null, 1=Bool, 2=I64, 3=F64, 4=Heap, 5=Undef). Used by
/// ssa_lower dispatch sites that need to branch on the runtime
/// type of an Any.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_unbox_tag(v: AnyValue) -> i64 {
    if is_int32(v) {
        AnySlotTag::I64 as i64
    } else if is_double(v) {
        AnySlotTag::F64 as i64
    } else if is_null(v) {
        AnySlotTag::Null as i64
    } else if is_undefined(v) {
        AnySlotTag::Undef as i64
    } else if is_bool(v) {
        AnySlotTag::Bool as i64
    } else if is_short_str(v) || is_cell(v) {
        // Step 8c — ShortStr legacy-pair coercion. ssa_lower's
        // pair-shaped consumers (`any_to_str` / `box_from_pair` /
        // `any_payload_rc_inc` / etc.) only know `Heap` + ptr;
        // ShortStr inline-encoded strings get materialized to a
        // Heap+Str pointer inside `__torajs_anyv_unbox_value` and
        // here the tag reports `Heap` so the downstream pair-
        // dispatch hits the Str arm. Future polish (8d/8e) routes
        // ssa_lower's Any pair sites through Any-shaped helpers so
        // the materialize cost goes away.
        AnySlotTag::Heap as i64
    } else {
        // Defensive — `v == 0` (uninitialized slot) treated as Null.
        AnySlotTag::Null as i64
    }
}

/// Decode an [`AnyValue`] to the legacy raw-`i64` value shape
/// the boxed-pair callers consumed:
///
/// - `Int32` → sign-extended to i64
/// - `f64`   → IEEE 754 bits
/// - `Bool`  → 0 / 1
/// - `Null` / `Undef` → 0
/// - `Cell`  → pointer as i64 (top 16 bits zero, low 48 bits VA)
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_unbox_value(v: AnyValue) -> i64 {
    if is_int32(v) {
        as_int32(v) as i64
    } else if is_double(v) {
        as_double(v).to_bits() as i64
    } else if is_null(v) || is_undefined(v) {
        0
    } else if is_bool(v) {
        if as_bool(v) { 1 } else { 0 }
    } else if is_short_str(v) {
        // Step 8c — ShortStr materialize for legacy pair API. The
        // companion `__torajs_anyv_unbox_tag` already reported
        // `Heap` for this `v`; here we have to hand the caller a
        // valid `*mut Str` pointer (refcount=1, owned). Materialize
        // bytes through the same `__torajs_str_alloc` path the
        // 8b-C shim helpers use; caller's existing pair-drop path
        // (`any_payload_drop` / `consume_any`) reclaims the rc.
        // SAFETY: is_short_str asserted; materialize produces a
        // freshly-owned Heap+Str.
        unsafe { materialize_short_str(v) as i64 }
    } else if is_cell(v) {
        v as i64
    } else {
        0
    }
}

/// Owned variant of [`__torajs_anyv_unbox_value`] — the caller
/// receives its own stake on the decoded payload:
///
/// - `Cell` → pointer + rc_inc (the pair carries a +1 the
///   consumer's storage keeps, mirroring what the separate
///   `any_payload_rc_inc` follow-up call used to add).
/// - `ShortStr` → materialized Heap+Str (refcount=1 — the
///   materialization itself IS the caller's stake, so no
///   follow-up inc; this closes the pre-existing leak where
///   `unbox_value` + `payload_rc_inc` left the materialized
///   block at rc=2 with only one reclaiming drop).
/// - Inline tags → same raw value as `unbox_value`.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_unbox_value_owned(v: AnyValue) -> i64 {
    if is_cell(v) {
        payload_rc_inc(AnySlotTag::Heap as i64, v as i64);
        v as i64
    } else {
        __torajs_anyv_unbox_value(v)
    }
}

/// Retain the box's heap payload — cell → +1, every immediate
/// (ShortStr included) a no-op. RFC 20260708-closure-argv-face:
/// `return __torajs_arguments[i]` hands the caller a box borrowing
/// the materialized array's elem stake; this retain makes the box
/// self-owned so the array keeps its normal scope drop.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_retain(v: AnyValue) -> AnyValue {
    if is_cell(v) {
        payload_rc_inc(AnySlotTag::Heap as i64, v as i64);
    }
    v
}

/// Settle the temporary a borrow-shaped [`__torajs_anyv_unbox_value`]
/// may have created: when `v` was a ShortStr the decode
/// materialized a fresh refcount=1 Heap+Str (`raw`), which the
/// pair-consuming helper only borrowed — the caller emits this
/// after the consuming call to reclaim that stake. For every
/// non-ShortStr `v` (heap cells included) this is a tag test +
/// no-op, keeping the true-heap fast path free of rc traffic.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_unbox_settle(v: AnyValue, raw: i64) {
    if is_short_str(v) {
        // SAFETY: for a ShortStr input, `raw` is the freshly-owned
        // materialized Str `__torajs_anyv_unbox_value` returned.
        unsafe { drop_materialized_str(raw as *mut u8) };
    }
}

mod incr;
mod pair;
pub use incr::*;
pub use pair::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nanbox::{box_double, box_int32};

    #[test]
    fn anyv_box_helpers_match_constants() {
        assert_eq!(__torajs_anyv_box_i32(0), box_int32(0));
        assert_eq!(__torajs_anyv_box_i32(-1), box_int32(-1));
        assert_eq!(__torajs_anyv_box_f64(3.14), box_double(3.14));
        assert_eq!(__torajs_anyv_box_bool(0i64), VALUE_FALSE);
        assert_eq!(__torajs_anyv_box_bool(1i64), VALUE_TRUE);
        assert_eq!(__torajs_anyv_box_bool(-1i64), VALUE_TRUE);
        assert_eq!(__torajs_anyv_null(), VALUE_NULL);
        assert_eq!(__torajs_anyv_undefined(), VALUE_UNDEFINED);
    }

    #[test]
    fn anyv_box_pointer_null_yields_null_sentinel() {
        unsafe {
            assert_eq!(__torajs_anyv_box_pointer(std::ptr::null_mut()), VALUE_NULL);
        }
    }

    #[test]
    fn anyv_box_i64_fits_i32() {
        for x in [0i64, 1, -1, 42, i32::MAX as i64, i32::MIN as i64] {
            let v = __torajs_anyv_box_i64(x);
            assert!(is_int32(v), "{x} should round-trip as Int32");
            assert_eq!(as_int32(v) as i64, x);
        }
    }

    #[test]
    fn anyv_box_i64_overflows_to_f64() {
        for x in [i32::MAX as i64 + 1, i32::MIN as i64 - 1, i64::MAX, i64::MIN] {
            let v = __torajs_anyv_box_i64(x);
            assert!(is_double(v), "{x} should promote to f64");
            assert!(!is_int32(v));
        }
    }

    #[test]
    fn anyv_box_from_pair_dispatch() {
        unsafe {
            assert_eq!(__torajs_anyv_box_from_pair(0, 0), VALUE_NULL);
            assert_eq!(__torajs_anyv_box_from_pair(1, 0), VALUE_FALSE);
            assert_eq!(__torajs_anyv_box_from_pair(1, 1), VALUE_TRUE);
            let v = __torajs_anyv_box_from_pair(2, 42);
            assert!(is_int32(v));
            assert_eq!(as_int32(v), 42);
            let v = __torajs_anyv_box_from_pair(3, 3.14f64.to_bits() as i64);
            assert!(is_double(v));
            assert_eq!(as_double(v), 3.14);
            assert_eq!(__torajs_anyv_box_from_pair(4, 0), VALUE_NULL);
            assert_eq!(__torajs_anyv_box_from_pair(5, 0), VALUE_UNDEFINED);
            assert_eq!(__torajs_anyv_box_from_pair(99, 0), VALUE_NULL);
        }
    }

    #[test]
    fn anyv_unbox_tag_for_each_kind() {
        assert_eq!(__torajs_anyv_unbox_tag(VALUE_NULL), AnySlotTag::Null as i64);
        assert_eq!(
            __torajs_anyv_unbox_tag(VALUE_UNDEFINED),
            AnySlotTag::Undef as i64
        );
        assert_eq!(__torajs_anyv_unbox_tag(VALUE_TRUE), AnySlotTag::Bool as i64);
        assert_eq!(
            __torajs_anyv_unbox_tag(VALUE_FALSE),
            AnySlotTag::Bool as i64
        );
        assert_eq!(
            __torajs_anyv_unbox_tag(box_int32(42)),
            AnySlotTag::I64 as i64
        );
        assert_eq!(
            __torajs_anyv_unbox_tag(box_double(3.14)),
            AnySlotTag::F64 as i64
        );
    }

    #[test]
    fn anyv_unbox_value_for_each_kind() {
        assert_eq!(__torajs_anyv_unbox_value(VALUE_NULL), 0);
        assert_eq!(__torajs_anyv_unbox_value(VALUE_UNDEFINED), 0);
        assert_eq!(__torajs_anyv_unbox_value(VALUE_TRUE), 1);
        assert_eq!(__torajs_anyv_unbox_value(VALUE_FALSE), 0);
        assert_eq!(__torajs_anyv_unbox_value(box_int32(42)), 42);
        assert_eq!(__torajs_anyv_unbox_value(box_int32(-7)), -7);
        let bits = __torajs_anyv_unbox_value(box_double(3.14));
        assert_eq!(f64::from_bits(bits as u64), 3.14);
    }

    #[test]
    fn anyv_unbox_value_owned_matches_unbox_for_inline_tags() {
        for v in [
            VALUE_NULL,
            VALUE_UNDEFINED,
            VALUE_TRUE,
            VALUE_FALSE,
            box_int32(42),
            box_int32(-7),
            box_double(3.14),
        ] {
            assert_eq!(
                __torajs_anyv_unbox_value_owned(v),
                __torajs_anyv_unbox_value(v)
            );
        }
    }

    #[test]
    fn anyv_unbox_value_owned_bumps_cell_refcount() {
        let mut cell = torajs_rc::HeapHeader::new(torajs_rc::Tag::Str);
        let ptr = &mut cell as *mut torajs_rc::HeapHeader;
        let any = unsafe { __torajs_anyv_box_pointer(ptr as *mut c_void) };
        let initial = cell.refcount;
        let raw = __torajs_anyv_unbox_value_owned(any);
        assert_eq!(raw, ptr as i64);
        assert_eq!(cell.refcount, initial + 1);
    }

    #[test]
    fn anyv_unbox_settle_no_op_on_non_short_str() {
        let mut cell = torajs_rc::HeapHeader::new(torajs_rc::Tag::Str);
        let ptr = &mut cell as *mut torajs_rc::HeapHeader;
        let any = unsafe { __torajs_anyv_box_pointer(ptr as *mut c_void) };
        let initial = cell.refcount;
        __torajs_anyv_unbox_settle(any, ptr as i64);
        assert_eq!(cell.refcount, initial, "cell input must not be dropped");
        __torajs_anyv_unbox_settle(box_int32(9), 9);
        __torajs_anyv_unbox_settle(VALUE_NULL, 0);
    }

    #[test]
    fn anyv_box_from_pair_round_trip_with_unbox() {
        unsafe {
            for (t, v) in [(0i64, 0i64), (1, 1), (2, 42), (3, 0), (5, 0)] {
                let any = __torajs_anyv_box_from_pair(t, v);
                let t2 = __torajs_anyv_unbox_tag(any);
                let v2 = __torajs_anyv_unbox_value(any);
                if t == 3 {
                    assert_eq!(t2, 3, "F64 tag should round-trip");
                } else {
                    assert_eq!(t2, t, "tag {t} → {t2}");
                }
                assert_eq!(v2, v, "value mismatch for tag {t}");
            }
        }
    }
}
