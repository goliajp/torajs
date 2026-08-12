//! BigInt lanes of the any-tier numeric kernels (rotation 241 —
//! sibling of [`crate::arith`], which grew past the 500-line file
//! cap when the lanes landed).
//!
//! ES splits every two-operand numeric op with ToNumeric (§13.6 /
//! §13.15.3 / §13.12): a BigInt pair rides the `torajs-bigint`
//! kernels, a BigInt beside a non-BigInt throws the mixed-types
//! TypeError, and no BigInt at all falls through to the Number
//! lane. Pre-fix a BigInt cell fell into `any_to_number`'s
//! OrdinaryToPrimitive object machinery — UB over a non-object
//! layout (silent no-output or SIGSEGV by binary layout).
//!
//! Comparison is NOT here: mixed BigInt/Number comparison is legal
//! (§7.2.13 mathematical value), so [`crate::compare`] carries its
//! own `bigint_side_cmp` and only shares [`heap_bigint_ptr`].

use std::ffi::c_void;

use torajs_rc::AnySlotTag;

use crate::arith_bitwise::BitwiseOp;
use crate::loose_eq::bigint_ffi;
use crate::nanbox::AnyValue;

/// The `(tag, value)` pair's BigInt cell pointer, when it is one.
/// The Heap-tag NULL check mirrors `any_to_number`'s.
#[inline]
pub(crate) unsafe fn heap_bigint_ptr(tag: i64, value: i64) -> Option<*const c_void> {
    if tag != AnySlotTag::Heap as i64 || value == 0 {
        return None;
    }
    // SAFETY: caller invariant — Heap value is a live heap header.
    let h = unsafe { &*(value as *const torajs_rc::HeapHeader) };
    if matches!(h.tag(), torajs_rc::Tag::BigInt) {
        Some(value as *const c_void)
    } else {
        None
    }
}

/// ToNumeric dispatch for a two-operand numeric op: both BigInt →
/// `Some` of the bigint-kernel result boxed as a fresh owned Heap
/// cell; one BigInt beside a non-BigInt → the spec's mixed-types
/// TypeError (pending throw + `Some(undefined)` placeholder for
/// the caller's throw check); no BigInt → `None`, the caller
/// proceeds down its Number lane. `kernel` is the matching
/// `__torajs_bigint_*` two-pointer entry; a NULL kernel result
/// (div/mod by 0n, pow negative exponent — the kernel recorded its
/// own RangeError) also answers the undefined placeholder.
pub(crate) unsafe fn try_bigint_pair(
    lt: i64,
    lv: i64,
    rt: i64,
    rv: i64,
    kernel: unsafe extern "C" fn(*const c_void, *const c_void) -> *mut u8,
) -> Option<AnyValue> {
    // SAFETY: caller invariant — propagated.
    let l_big = unsafe { heap_bigint_ptr(lt, lv) };
    let r_big = unsafe { heap_bigint_ptr(rt, rv) };
    match (l_big, r_big) {
        (Some(a), Some(b)) => {
            // SAFETY: both are live BigInt cells; the kernel mints
            // a fresh rc=1 cell (or NULL with a pending throw).
            let out = unsafe { kernel(a, b) };
            if out.is_null() {
                return Some(crate::nanbox::VALUE_UNDEFINED);
            }
            Some(crate::nanbox::box_void_ptr(out as *mut c_void))
        }
        (None, None) => None,
        _ => {
            // SAFETY: pure FFI throw-record.
            unsafe {
                crate::member_set::__torajs_throw_type_error(
                    c"Cannot mix BigInt and other types, use explicit conversions".as_ptr(),
                );
            }
            Some(crate::nanbox::VALUE_UNDEFINED)
        }
    }
}

/// The BigInt leg of [`crate::arith::any_bitwise`] — §13.12 over
/// §6.1.6.2's full-width BigInt bitwise/shift ops. `>>>` has no
/// BigInt form (§6.1.6.2.9): the signed-shift kernel result is
/// released and the TypeError recorded instead.
pub(crate) unsafe fn try_bitwise_bigint(
    bit_op: BitwiseOp,
    lt: i64,
    lv: i64,
    rt: i64,
    rv: i64,
) -> Option<AnyValue> {
    let kernel = match bit_op {
        BitwiseOp::BitAnd => bigint_ffi::__torajs_bigint_and,
        BitwiseOp::BitOr => bigint_ffi::__torajs_bigint_or,
        BitwiseOp::BitXor => bigint_ffi::__torajs_bigint_xor,
        BitwiseOp::Shl => bigint_ffi::__torajs_bigint_shl,
        BitwiseOp::Shr | BitwiseOp::UShr => bigint_ffi::__torajs_bigint_shr,
    };
    // SAFETY: caller invariant — propagated.
    let hit = unsafe { try_bigint_pair(lt, lv, rt, rv, kernel) }?;
    if matches!(bit_op, BitwiseOp::UShr) && !crate::nanbox::is_undefined(hit) {
        // SAFETY: hit is the kernel's fresh owned cell.
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(hit) };
        // SAFETY: pure FFI throw-record.
        unsafe {
            crate::member_set::__torajs_throw_type_error(
                c"BigInts have no unsigned right shift, use >> instead".as_ptr(),
            );
        }
        return Some(crate::nanbox::VALUE_UNDEFINED);
    }
    Some(hit)
}

/// The BigInt leg of [`crate::arith::any_bitnot`] — §6.1.6.2.2
/// full-width `~`.
pub(crate) unsafe fn try_bitnot_bigint(tag: i64, value: i64) -> Option<AnyValue> {
    // SAFETY: caller invariant — propagated.
    let p = unsafe { heap_bigint_ptr(tag, value) }?;
    // SAFETY: p is a live BigInt cell; the kernel mints rc=1.
    let out = unsafe { bigint_ffi::__torajs_bigint_not(p) };
    if out.is_null() {
        return Some(crate::nanbox::VALUE_UNDEFINED);
    }
    Some(crate::nanbox::box_void_ptr(out as *mut c_void))
}

/// The BigInt leg of the any-tier unary minus — §6.1.6.2.1: unary
/// minus is LEGAL on a BigInt (unlike ToNumber, §7.1.4 step 2), so
/// the `0 - x` Number-lane identity the lowering uses everywhere
/// else must not run (its mixed-pair arm would throw). `None` = not
/// a BigInt, the caller's Number lane proceeds.
pub(crate) unsafe fn try_unary_neg_bigint(tag: i64, value: i64) -> Option<AnyValue> {
    // SAFETY: caller invariant — propagated.
    let p = unsafe { heap_bigint_ptr(tag, value) }?;
    // SAFETY: p is a live BigInt cell; the kernel mints rc=1.
    let out = unsafe { bigint_ffi::__torajs_bigint_neg(p) };
    if out.is_null() {
        return Some(crate::nanbox::VALUE_UNDEFINED);
    }
    Some(crate::nanbox::box_void_ptr(out as *mut c_void))
}

/// Any-tier unary minus: the BigInt leg above, else the Number
/// lane's `0 - x` through [`crate::arith::any_arith`] (whose
/// ToNumber records the Symbol / mixed rejects as pending throws —
/// the lowering emits the throw check).
pub(crate) unsafe fn any_unary_neg(tag: i64, value: i64) -> AnyValue {
    // SAFETY: caller invariant — propagated.
    if let Some(v) = unsafe { try_unary_neg_bigint(tag, value) } {
        return v;
    }
    // SAFETY: same contract; 0=Sub, ANY_I64 tag = 2 (ssa_lower wire
    // format).
    unsafe { crate::arith::any_arith(0, AnySlotTag::I64 as i64, 0, tag, value) }
}
