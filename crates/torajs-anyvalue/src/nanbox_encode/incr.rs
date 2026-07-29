//! ES §13.4.4 / §13.4.5 — the update expressions (`x++`, `x--`) over
//! an `any`-typed slot.
//!
//! The typed lanes lower `x++` inline: load, add one at the slot's own
//! width, store. An `any` slot cannot, because §13.4.4.1 puts a
//! ToNumeric between the load and the add — the operand may be a
//! string, a boolean, or an object with `valueOf`, and it must be
//! coerced exactly ONCE (a second coercion would call `valueOf` twice,
//! which is observable). It also decides which numeric domain the add
//! happens in: a BigInt increments as a BigInt, everything else as a
//! Number, and the two must not be mixed.
//!
//! So the whole read-modify-write lives here rather than being spread
//! across the lowering: the slot pointer comes in, the reference
//! counting on the replaced value stays next to the store, and the
//! caller receives the old value already coerced — which is what the
//! expression evaluates to (`let s = "5"; s++` answers the NUMBER 5,
//! not the string).

use core::ffi::c_void;

use super::*;
use crate::arith_bigint::heap_bigint_ptr;
use crate::loose_eq::bigint_ffi;

/// `ArithOp::Sub`'s wire value in [`crate::arith::any_arith`]'s op
/// encoding — the update step rides subtraction in both directions
/// (`x - -1` for `++`, `x - 1` for `--`) because it is the pure
/// numeric operator: `+` would concatenate when the operand coerces
/// to a string.
const OP_SUB: i64 = 0;

/// Read `*slot`, coerce it per ToNumeric, write back the incremented
/// (or decremented) value, and answer the coerced OLD value.
///
/// `is_inc` is 1 for `++` and 0 for `--`.
///
/// The returned value is owned by the caller. The value previously in
/// the slot is released here.
///
/// # Safety
///
/// `slot` must point to a live, initialised `AnyValue` that this call
/// is allowed to overwrite.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_incr_slot(slot: *mut AnyValue, is_inc: i64) -> AnyValue {
    // SAFETY: caller invariant — slot is live and initialised.
    let cur = unsafe { *slot };
    let tag = __torajs_anyv_unbox_tag(cur);
    let value = __torajs_anyv_unbox_value(cur);

    // SAFETY: tag / value came out of a live AnyValue.
    let (old, new) = match unsafe { heap_bigint_ptr(tag, value) } {
        // BigInt lane — ToNumeric is the identity, and the step must
        // stay in the BigInt domain (§6.1.6.2). Mixing in a Number 1
        // here is exactly the TypeError the arith kernel raises for
        // `1n - 1`, so the operand is minted as a BigInt.
        Some(cell) => {
            // SAFETY: pure FFI; mints a fresh rc=1 cell.
            let one = unsafe { bigint_ffi::__torajs_bigint_from_number(1.0) };
            if one.is_null() {
                return VALUE_UNDEFINED;
            }
            let kernel = if is_inc != 0 {
                bigint_ffi::__torajs_bigint_add
            } else {
                bigint_ffi::__torajs_bigint_sub
            };
            // SAFETY: both are live BigInt cells.
            let out = unsafe { kernel(cell, one as *const c_void) };
            // SAFETY: the minted operand is ours to release.
            unsafe { bigint_ffi::__torajs_bigint_drop(one as *mut c_void) };
            if out.is_null() {
                return VALUE_UNDEFINED;
            }
            // cur is a live BigInt AnyValue; retain takes the caller's
            // stake in the value we are about to answer.
            let old = __torajs_anyv_retain(cur);
            (old, box_void_ptr(out as *mut c_void))
        }
        // Number lane — `x - 0` performs ToNumeric once and boxes the
        // result under the kernel's own i64 / f64 convention, so the
        // old value never needs a second coercion. The step then runs
        // against that already-numeric value.
        None => {
            // SAFETY: tag / value came out of a live AnyValue.
            let old = unsafe { any_arith(OP_SUB, tag, value, AnySlotTag::I64 as i64, 0) };
            let step: i64 = if is_inc != 0 { -1 } else { 1 };
            let old_tag = __torajs_anyv_unbox_tag(old);
            let old_value = __torajs_anyv_unbox_value(old);
            // SAFETY: old is a live numeric AnyValue.
            let new =
                unsafe { any_arith(OP_SUB, old_tag, old_value, AnySlotTag::I64 as i64, step) };
            (old, new)
        }
    };

    // SAFETY: cur is the slot's own stake, replaced by `new`.
    unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(cur) };
    // SAFETY: caller invariant — slot is writable.
    unsafe { *slot = new };
    old
}
