//! Equality / relational / arithmetic shims over [`AnyValue`] —
//! `strict_eq`, `same_value`, `compare`, `arith` and `add`. Split out
//! of the parent under the 500-line file discipline (rotation 146);
//! a child module reaches its parent's private items, so the decode
//! helpers and the extern block stay private with no visibility churn.
//!
//! Verbatim move — the spec citations, the numeric-row joins and the
//! `#[unsafe(no_mangle)]` symbol names are unchanged.

use core::ffi::c_void;

use super::*;

// ============================================================
// Strict equality
// ============================================================

/// Strict `===` per ES §7.2.13 over [`AnyValue`] operands.
///
/// Identity match handles primitives + heap-pointer identity in
/// one cmp; only the cell-string-equality path needs heap reads.
///
/// JS-spec quirks:
/// - `NaN === NaN` is `false` (even when bits identical).
/// - `+0 === -0` is `true` (they have different bits but compare
///   equal under f64 `==`).
/// - `1 === 1.0` is `true` (cross-type numeric coerces).
///
/// # Safety
///
/// Cell-case operands must point to valid heap objects.
/// §7.2.10 SameValue — strict-eq with the two numeric corrections:
/// `SameValue(NaN, NaN)` is true and `SameValue(+0, -0)` is false.
/// The §10.1.6.3 non-configurable redefine gate is the consumer
/// (an exact-bits approximation wrongly rejected a same-VALUE Str
/// redefine — two "abcd" cells have different pointers).
///
/// # Safety
/// Cell-case operands must point to valid heap objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_same_value(l: AnyValue, r: AnyValue) -> bool {
    unsafe {
        // BOTH numeric representations join the numeric row — an
        // integer literal packs as int32 while -0 packs as a
        // double, and the mixed pair must still discriminate ±0
        // (SameValue(-0, +0) is false; the first cut fell through
        // to strict-eq's cross-type row and answered true —
        // test262 defineProperty 4-64/65/86 appeared).
        let l_num = is_double(l) || is_int32(l);
        let r_num = is_double(r) || is_int32(r);
        if l_num && r_num {
            let a = if is_double(l) {
                as_double(l)
            } else {
                as_int32(l) as f64
            };
            let b = if is_double(r) {
                as_double(r)
            } else {
                as_int32(r) as f64
            };
            if a.is_nan() && b.is_nan() {
                return true;
            }
            if a == 0.0 && b == 0.0 {
                return a.is_sign_negative() == b.is_sign_negative();
            }
            return a == b;
        }
        // The Str byte-equality and identity rows match strict-eq.
        __torajs_anyv_strict_eq(l, r)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_strict_eq(l: AnyValue, r: AnyValue) -> bool {
    // Identity fast path — bit-equal values are usually equal.
    if l == r {
        // NaN exception: bit-equal NaN compares unequal in JS.
        if is_double(l) {
            return !as_double(l).is_nan();
        }
        return true;
    }
    // f64(0.0) vs f64(-0.0): different bits, equal in JS.
    if is_double(l) && is_double(r) {
        return as_double(l) == as_double(r);
    }
    // Cross-type numeric: i32 vs f64 → coerce both to f64.
    if is_int32(l) && is_double(r) {
        return (as_int32(l) as f64) == as_double(r);
    }
    if is_double(l) && is_int32(r) {
        return as_double(l) == (as_int32(r) as f64);
    }
    // Cell pair: Str↔Str does byte-equality via the C-side
    // __torajs_str_eq (matches the existing AnyBox path).
    if is_cell(l) && is_cell(r) {
        let lp = as_pointer(l);
        let rp = as_pointer(r);
        // SAFETY: both cells non-null per is_cell.
        let (lh, rh) = unsafe { (&*lp, &*rp) };
        if matches!(lh.tag(), Tag::Str) && matches!(rh.tag(), Tag::Str) {
            // SAFETY: both heap headers are Tag::Str — __torajs_str_eq
            // reads the matching Str layout.
            return unsafe { __torajs_str_eq(lp as *const u8, rp as *const u8) != 0 };
        }
        // §7.2.15 step 3 → §7.2.12: BigInt === BigInt compares
        // mathematical values, not identity (RFC
        // 20260713-loose-eq-substrate blade 1 — distinct 1n cells
        // used to answer false here).
        if matches!(lh.tag(), Tag::BigInt) && matches!(rh.tag(), Tag::BigInt) {
            // SAFETY: both cells are Tag::BigInt blocks.
            return unsafe {
                crate::loose_eq::bigint_ffi::__torajs_bigint_eq(
                    lp as *const c_void,
                    rp as *const c_void,
                ) != 0
            };
        }
        // Other heap pairs: pointer identity only — already
        // covered by `l == r` early-exit above.
        return false;
    }
    // Step 8b-C: cross-type ShortStr × Heap+Str byte-compare via
    // materialize. Same-ShortStr × Same-ShortStr already covered by
    // the identity fast path (`l == r`). Different-ShortStr × Diff-
    // ShortStr falls through to `false` per the cell-pair-required
    // branch — correct: distinct ShortStr bits = distinct bytes.
    // ShortStr × non-Str-Heap returns false (string !== object). All
    // non-string primitive cross-types (ShortStr vs i32 / f64 / bool
    // / null / undef) also fall through to `false` — correct per ES.
    if is_short_str(l) && is_cell(r) {
        let rp = as_pointer(r);
        // SAFETY: r is a cell — non-null per is_cell.
        let rh = unsafe { &*rp };
        if matches!(rh.tag(), Tag::Str) {
            // SAFETY: materialize widens l to a fresh refcount=1 Str.
            let ls = unsafe { materialize_short_str(l) };
            // SAFETY: ls is Heap+Str layout, r is Heap+Str layout.
            let eq = unsafe { __torajs_str_eq(ls, rp as *const u8) != 0 };
            // SAFETY: drop the temporary.
            unsafe { drop_materialized_str(ls) };
            return eq;
        }
        return false;
    }
    if is_cell(l) && is_short_str(r) {
        let lp = as_pointer(l);
        // SAFETY: l is a cell — non-null per is_cell.
        let lh = unsafe { &*lp };
        if matches!(lh.tag(), Tag::Str) {
            // SAFETY: materialize widens r to a fresh refcount=1 Str.
            let rs = unsafe { materialize_short_str(r) };
            // SAFETY: both Heap+Str layout.
            let eq = unsafe { __torajs_str_eq(lp as *const u8, rs) != 0 };
            // SAFETY: drop the temporary.
            unsafe { drop_materialized_str(rs) };
            return eq;
        }
        return false;
    }
    false
}

// ============================================================
// Compare / arithmetic — 7b delegates to boxed inner helpers
//
// The inner helpers (any_compare / any_arith / any_add) still
// take `(tag, value)` pairs. 7b decodes the AnyValue immediates
// back to that shape, calls into them, and re-encodes the result.
// 7c-7e replace the inner helpers with AnyValue-direct logic
// (zero box alloc); these decode/re-encode helpers then go away.
// ============================================================

/// Relational compare (`<`, `<=`, `>`, `>=`) per ES §7.2.13.
/// `op`: 0=Lt, 1=Le, 2=Gt, 3=Ge.
///
/// # Safety
///
/// Cell-case operands must point to valid heap objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_compare(op: i64, l: AnyValue, r: AnyValue) -> bool {
    // Step 8b-C: ShortStr operands materialize to Heap+Str before
    // entering the legacy `(tag, value)` decoder path. Temporaries
    // are dropped after the inner helper returns.
    // SAFETY: materialize_if_short upholds the Heap+Str invariant.
    let (l_eff, l_tmp) = unsafe { materialize_if_short(l) };
    let (r_eff, r_tmp) = unsafe { materialize_if_short(r) };
    let (lt, lv) = decode_to_tag_value(l_eff);
    let (rt, rv) = decode_to_tag_value(r_eff);
    // SAFETY: decode_to_tag_value preserves the cell-pointer
    // validity; any_compare is documented to handle the
    // `(Heap, valid_ptr)` case.
    let result = unsafe { any_compare(op, lt, lv, rt, rv) };
    // SAFETY: drop the materialized temporaries (if any).
    if let Some(p) = l_tmp {
        unsafe { drop_materialized_str(p) };
    }
    if let Some(p) = r_tmp {
        unsafe { drop_materialized_str(p) };
    }
    result
}

/// Arithmetic dispatch (`-`, `*`, `/`, `%`) per ES §13.6-§13.9.
/// `op`: 0=Sub, 1=Mul, 2=Div, 3=Mod. Returns a fresh [`AnyValue`].
///
/// # Safety
///
/// Cell-case operands must point to valid heap objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_arith(op: i64, l: AnyValue, r: AnyValue) -> AnyValue {
    // Step 8b-C: materialize ShortStr operands. Arithmetic on
    // strings coerces to numbers per ES so any_arith's ToNumber
    // path will parse the bytes.
    // SAFETY: as compare.
    let (l_eff, l_tmp) = unsafe { materialize_if_short(l) };
    let (r_eff, r_tmp) = unsafe { materialize_if_short(r) };
    let (lt, lv) = decode_to_tag_value(l_eff);
    let (rt, rv) = decode_to_tag_value(r_eff);
    // SAFETY: as above.
    let result = unsafe { any_arith(op, lt, lv, rt, rv) };
    if let Some(p) = l_tmp {
        unsafe { drop_materialized_str(p) };
    }
    if let Some(p) = r_tmp {
        unsafe { drop_materialized_str(p) };
    }
    result
}

/// `+` per ES §13.15.3 — numeric addition or string concat.
/// Returns a fresh [`AnyValue`].
///
/// # Safety
///
/// Cell-case operands must point to valid heap objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_add(l: AnyValue, r: AnyValue) -> AnyValue {
    // Step 8b-C: materialize ShortStr operands. `+` is dual-mode
    // (numeric add or string concat); both modes need the operand
    // as a Heap+Str pointer when the string branch fires.
    // SAFETY: as compare.
    let (l_eff, l_tmp) = unsafe { materialize_if_short(l) };
    let (r_eff, r_tmp) = unsafe { materialize_if_short(r) };
    let (lt, lv) = decode_to_tag_value(l_eff);
    let (rt, rv) = decode_to_tag_value(r_eff);
    // SAFETY: as above.
    let result = unsafe { any_add(lt, lv, rt, rv) };
    if let Some(p) = l_tmp {
        unsafe { drop_materialized_str(p) };
    }
    if let Some(p) = r_tmp {
        unsafe { drop_materialized_str(p) };
    }
    result
}

// ============================================================
// Internal decode/encode bridges
// ============================================================

/// Decode an [`AnyValue`] immediate to the legacy `(tag, value)`
/// pair shape that the un-rewritten inner helpers consume.
/// Heap-pointer values pass through with `tag = Heap` so the
/// helper's `Heap` branch is reached.
///
/// Step 8b-C ShortStr handling: callers that pipe through
/// [`any_compare`] / [`any_arith`] / [`any_add`] must materialize
/// ShortStr operands first (via
/// [`materialize_if_short`](crate::nanbox_ffi_materialize::materialize_if_short));
/// the inner helpers expect `(Heap, *mut HeapHeader)` pairs, not
/// inline ShortStr bits. This function itself routes ShortStr to
/// the defensive `(Null, 0)` fallback — UB if it ever fires
/// without materialize first.
pub(super) fn decode_to_tag_value(v: AnyValue) -> (i64, i64) {
    if is_int32(v) {
        (AnySlotTag::I64 as i64, as_int32(v) as i64)
    } else if is_double(v) {
        (AnySlotTag::F64 as i64, as_double(v).to_bits() as i64)
    } else if is_null(v) {
        (AnySlotTag::Null as i64, 0)
    } else if is_undefined(v) {
        (AnySlotTag::Undef as i64, 0)
    } else if is_bool(v) {
        (AnySlotTag::Bool as i64, if as_bool(v) { 1 } else { 0 })
    } else if is_cell(v) {
        (AnySlotTag::Heap as i64, v as i64)
    } else {
        (AnySlotTag::Null as i64, 0)
    }
}
