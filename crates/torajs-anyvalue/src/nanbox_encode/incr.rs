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
    // SAFETY: cur is a live AnyValue.
    let Some((old, new)) = (unsafe { step_numeric(cur, is_inc) }) else {
        return VALUE_UNDEFINED;
    };
    // SAFETY: cur is the slot's own stake, replaced by `new`.
    unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(cur) };
    // SAFETY: caller invariant — slot is writable.
    unsafe { *slot = new };
    old
}

/// The value-shaped face of the same §13.4.4.1 step — no slot, so the
/// member update lane can compose it between its own GetV and PutValue
/// (`(o: any).f++` has no slot pointer to hand over: the store must go
/// back through the member-set kernel with its accessor / refusal
/// semantics). Coerces `cur` per ToNumeric exactly once, writes the
/// coerced OLD value to `old_out`, and answers the stepped NEW value.
///
/// Both `*old_out` and the returned value are owned by the caller;
/// `cur` is borrowed. A failed BigInt mint answers undefined in both
/// positions (mirrors the slot form's bail).
///
/// # Safety
///
/// `cur` is a live AnyValue; `old_out` points at writable storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_incr_value(
    cur: AnyValue,
    is_inc: i64,
    old_out: *mut AnyValue,
) -> AnyValue {
    // SAFETY: cur is a live AnyValue.
    let Some((old, new)) = (unsafe { step_numeric(cur, is_inc) }) else {
        // SAFETY: caller invariant — old_out is writable.
        unsafe { *old_out = VALUE_UNDEFINED };
        return VALUE_UNDEFINED;
    };
    // SAFETY: caller invariant — old_out is writable.
    unsafe { *old_out = old };
    new
}

/// ToNumeric + step core shared by the slot and value faces above:
/// answers `(old, new)`, both owned, with the coercion run exactly
/// once and the step taken in the operand's own numeric domain
/// (§6.1.6.2 — a BigInt steps as a BigInt, everything else as a
/// Number). `None` = a BigInt operand mint failed; callers bail
/// without storing.
///
/// # Safety
///
/// `cur` is a live AnyValue.
unsafe fn step_numeric(cur: AnyValue, is_inc: i64) -> Option<(AnyValue, AnyValue)> {
    let tag = __torajs_anyv_unbox_tag(cur);
    let value = __torajs_anyv_unbox_value(cur);

    // SAFETY: tag / value came out of a live AnyValue.
    let pair = match unsafe { heap_bigint_ptr(tag, value) } {
        // BigInt lane — ToNumeric is the identity, and the step must
        // stay in the BigInt domain (§6.1.6.2). Mixing in a Number 1
        // here is exactly the TypeError the arith kernel raises for
        // `1n - 1`, so the operand is minted as a BigInt.
        Some(cell) => {
            // SAFETY: pure FFI; mints a fresh rc=1 cell.
            let one = unsafe { bigint_ffi::__torajs_bigint_from_number(1.0) };
            if one.is_null() {
                return None;
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
                return None;
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
            // any_arith only borrows; a ShortStr operand materialized
            // an owned rc=1 Str in the unbox above (546-02 M1 family
            // — `let s = "1"; s++` leaked one per step).
            if crate::nanbox::is_short_str(cur) && value != 0 {
                // SAFETY: the materialization is ours to release.
                unsafe { crate::__torajs_value_drop_heap(value as *mut c_void) };
            }
            let step: i64 = if is_inc != 0 { -1 } else { 1 };
            let old_tag = __torajs_anyv_unbox_tag(old);
            let old_value = __torajs_anyv_unbox_value(old);
            // SAFETY: old is a live numeric AnyValue.
            let new =
                unsafe { any_arith(OP_SUB, old_tag, old_value, AnySlotTag::I64 as i64, step) };
            (old, new)
        }
    };
    Some(pair)
}
