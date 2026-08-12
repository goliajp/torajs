//! `(tag, value)`-pair entry points — ssa_lower's pair-shaped shims
//! over the AnyValue-immediate impls. Split out of the parent under
//! the 500-line file discipline (rotation 146); a child module reaches
//! its parent's private items, so the encode/decode helpers and the
//! extern block stay private with no visibility churn.
//!
//! Verbatim move — every `#[unsafe(no_mangle)]` symbol name and the
//! delegate bodies are unchanged.

use core::ffi::c_void;

use super::*;

// ============================================================
// Pair-arg shims — ssa_lower's `(tag, value)`-pair entry points.
// The inner `any_arith` / `any_add` impls in [`crate::arith`]
// return AnyValue immediates directly (Step 7f-D-2 rewrite); the
// pair shims are now one-line delegates.
// ============================================================

/// Pair-arg arithmetic: takes the legacy `(op, lt, lv, rt, rv)`
/// shape ssa_lower produces via `any_unbox_tag` +
/// `any_unbox_value`, returns a fresh [`AnyValue`].
///
/// # Safety
///
/// `lt` / `rt` ∈ `AnySlotTag`. If tag == Heap, value is 0 or a
/// valid `*mut HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_arith_pair(
    op: i64,
    lt: i64,
    lv: i64,
    rt: i64,
    rv: i64,
) -> AnyValue {
    // SAFETY: caller invariant on tag/value pairs.
    unsafe { any_arith(op, lt, lv, rt, rv) }
}

/// Pair-arg `+`: legacy `(lt, lv, rt, rv)` shape →
/// [`AnyValue`].
///
/// # Safety
///
/// Same as [`__torajs_anyv_arith_pair`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_add_pair(lt: i64, lv: i64, rt: i64, rv: i64) -> AnyValue {
    // SAFETY: caller invariant on tag/value pairs.
    unsafe { any_add(lt, lv, rt, rv) }
}

/// Pair-arg bitwise dispatch: takes the SSA-emitted
/// `(op, lt, lv, rt, rv)` shape per ES §13.12 — op ∈ {0=BitAnd,
/// 1=BitOr, 2=BitXor, 3=Shl, 4=Shr, 5=UShr}. Both operands
/// `ToNumber` → `ToInt32` (`ToUint32` on `>>>`'s LHS); returns a
/// fresh [`AnyValue`] boxed as I32 (fits) or F64 (`>>>` results in
/// `[2^31, 2^32)`).
///
/// RFC 20260716 刀 7 — closes the sweep-flagged pass→incompat
/// regression cluster where `new Boolean(x) & prim` sites got
/// rejected by the checker.
///
/// # Safety
///
/// `lt` / `rt` ∈ `AnySlotTag`. If tag == Heap, value is 0 or a
/// valid `*mut HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_bitwise_pair(
    op: i64,
    lt: i64,
    lv: i64,
    rt: i64,
    rv: i64,
) -> AnyValue {
    // SAFETY: caller invariant on tag/value pairs.
    unsafe { any_bitwise(op, lt, lv, rt, rv) }
}

/// Pair-arg unary `~` per ES §13.5.6. Operand `(tag, value)` pair
/// routes through `ToNumber` → `ToInt32` → `xor -1`. Result
/// always fits in i32.
///
/// # Safety
///
/// Same as [`__torajs_anyv_bitwise_pair`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_bitnot_pair(tag: i64, value: i64) -> AnyValue {
    // SAFETY: caller invariant on tag/value pair.
    unsafe { any_bitnot(tag, value) }
}

/// Pair-arg unary `-` per ES §13.5.5 — a BigInt operand negates
/// through its own kernel (§6.1.6.2.1, unary minus IS legal on
/// BigInt), everything else rides the Number lane's `0 - x`. The
/// `~` shim above got its BigInt leg first; this closes the same
/// family gap for `-` (the raw `any_arith(0 - x)` emission threw
/// the mixed-pair TypeError on a BigInt).
///
/// # Safety
///
/// Same as [`__torajs_anyv_bitwise_pair`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_unary_neg_pair(tag: i64, value: i64) -> AnyValue {
    // SAFETY: caller invariant on tag/value pair.
    unsafe { crate::arith_bigint::any_unary_neg(tag, value) }
}

/// Immediate-vs-pair strict equality. ssa_lower's array-includes
/// + class-tag dispatch paths produce one `AnyValue` immediate
/// (the heap-stored element) and one statically-decoded
/// `(tag, value)` pair (the literal needle). Reconstructs the
/// needle as an immediate and delegates to
/// [`__torajs_anyv_strict_eq`].
///
/// # Safety
///
/// `l` is a valid AnyValue bit pattern. `rt` ∈ `AnySlotTag`;
/// `rv` matches the SSA-layer packing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_strict_eq_imm_pair(l: AnyValue, rt: i64, rv: i64) -> bool {
    let r = rebuild_imm(rt, rv);
    // SAFETY: anyv_strict_eq's contract — cell-case operands valid.
    unsafe { __torajs_anyv_strict_eq(l, r) }
}

/// SameValueZero variant of [`__torajs_anyv_strict_eq_imm_pair`] —
/// ES §7.2.9: strict equality except `NaN` equals `NaN` (±0 already
/// equal under strict-eq). `Array.prototype.includes` (§23.1.3.16)
/// with an `any` needle over a typed-element receiver is the
/// consumer; indexOf / lastIndexOf keep the strict entry.
///
/// # Safety
///
/// Same as [`__torajs_anyv_strict_eq_imm_pair`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_svz_imm_pair(l: AnyValue, rt: i64, rv: i64) -> bool {
    let r = rebuild_imm(rt, rv);
    let l_nan = is_double(l) && as_double(l).is_nan();
    let r_nan = is_double(r) && as_double(r).is_nan();
    if l_nan && r_nan {
        return true;
    }
    // SAFETY: anyv_strict_eq's contract — cell-case operands valid.
    unsafe { __torajs_anyv_strict_eq(l, r) }
}

/// Reconstruct the SSA layer's statically-decoded `(tag, value)`
/// pair as an AnyValue immediate — shared by the strict-eq and
/// SameValueZero pair entries.
fn rebuild_imm(rt: i64, rv: i64) -> AnyValue {
    match rt {
        0 => VALUE_NULL,
        1 => {
            if rv != 0 {
                VALUE_TRUE
            } else {
                VALUE_FALSE
            }
        }
        2 => {
            if let Ok(n32) = i32::try_from(rv) {
                box_int32(n32)
            } else {
                box_double(rv as f64)
            }
        }
        3 => box_double(f64::from_bits(rv as u64)),
        4 => {
            if rv == 0 {
                VALUE_NULL
            } else {
                rv as u64
            }
        }
        5 => VALUE_UNDEFINED,
        _ => VALUE_NULL,
    }
}

/// Pair-arg relational compare per ES §7.2.13. Takes ssa_lower's
/// already-decoded `(op, lt, lv, rt, rv)` shape; routes through
/// the internal `any_compare` impl. Step 7f-A gap fill — the
/// canonical `_anyv_` compare entry point ssa_lower binds against
/// (7f-B/C rebinds), under the NaN-box AnyValue ABI.
///
/// # Safety
///
/// `lt` / `rt` ∈ `AnySlotTag`. If tag == Heap, value is 0 or a
/// valid `*mut HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_compare_pair(
    op: i64,
    lt: i64,
    lv: i64,
    rt: i64,
    rv: i64,
) -> bool {
    // SAFETY: caller invariant on tag/value pairs.
    unsafe { any_compare(op, lt, lv, rt, rv) }
}

/// Pair-arg heap-payload refcount bump. ssa_lower emits this at
/// every site that materializes an Any from a known `(tag, value)`
/// without going through `__torajs_anyv_box_from_pair` (because
/// the box won't be observable — e.g. the value is about to be
/// stashed into a typed storage that owns its own ref). Step 7f-A
/// gap fill — NaN-box pair-arg payload rc-inc entry point.
///
/// # Safety
///
/// If `tag == AnySlotTag::Heap as i64`, `value` must be null or
/// a valid `*mut HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_payload_rc_inc_pair(tag: i64, value: i64) {
    payload_rc_inc(tag, value);
}

/// Pair-arg ToString. Returns a freshly-owned Str pointer
/// (refcount=1) the caller must drop. Used by ssa_lower at every
/// implicit ToString site (template literals, `+` mixing string
/// and non-string operands, `console.log(any)`). Step 7f-A gap
/// fill — NaN-box pair-arg ToString entry point with the
/// `(tag, value) -> *mut c_void` signature.
///
/// # Safety
///
/// For `tag == Heap`, `value` is null or a valid `*mut
/// HeapHeader` pointing to a live heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_to_str_pair(tag: i64, value: i64) -> *mut c_void {
    // SAFETY: caller invariant on (tag, value) pair.
    unsafe { any_to_str(tag, value) }
}

/// Pair-arg ToString for the STRING-CONCAT position (§13.15.3
/// steps 1-2): an object operand runs ToPrimitive with the DEFAULT
/// hint first (valueOf → toString for ordinary objects; a Date maps
/// default to string order), and the primitive result stringifies.
/// The plain [`__torajs_anyv_to_str_pair`] is hint-string ToString
/// (template literals, String()) — using it in `+` ran toString
/// ahead of valueOf (`"" + {valueOf: …}` observable order). A
/// double-object ToPrimitive failure records the catchable
/// TypeError and answers an empty Str placeholder for the caller's
/// throw check to unwind.
///
/// # Safety
/// For `tag == Heap`, `value` is null or a valid `*mut HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_to_str_pair_prim(tag: i64, value: i64) -> *mut c_void {
    // SAFETY: caller invariant on (tag, value) pair.
    unsafe { crate::coerce::any_to_str_prim(tag, value) }
}
