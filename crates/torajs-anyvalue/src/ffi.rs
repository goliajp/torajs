//! `__torajs_any_*` extern "C" FFI shims — the legacy *AnyBox-shape
//! entry points ssa_lower binds against. **Step 7d-A atomic switch
//! (HEAD 18a3478 → next):** every entry below delegates to its
//! `__torajs_anyv_*` NaN-box-immediate sister in
//! [`crate::nanbox_ffi`] / [`crate::nanbox_encode`]. The `*c_void` /
//! `*AnyBox` parameter is bit-reinterpreted as `AnyValue` (`u64`);
//! at the LLVM ABI level both are 64-bit ptr-sized so the cast is
//! free. ssa_lower keeps its existing IR — it still emits
//! `Call __torajs_any_box(tag, value) -> Type::Any (ptr)` etc. —
//! but the pointer no longer references heap memory: it carries the
//! NaN-box bit-pattern in its bit positions.
//!
//! Why a delegate layer (rather than renaming ssa_lower's bindings):
//! the inverse is symmetrical and lets every other host crate
//! (torajs-meta / torajs-runtime) keep its existing `*const c_void`
//! API while transparently switching to NaN-box semantics. Step 7f
//! deletes both layers and ssa_lower binds straight at the anyv_*
//! sisters.
//!
//! Internal helpers (`any_to_str`, `any_to_number`, `any_compare`,
//! `payload_rc_inc`) live in their dedicated modules and are
//! exposed via `pub(crate)`.

use std::ffi::c_void;

use crate::coerce::{any_to_number, any_to_str};
use crate::compare::any_compare;
use crate::nanbox::AnyValue;
use crate::nanbox_encode::{
    __torajs_anyv_add_pair, __torajs_anyv_arith_pair, __torajs_anyv_box_from_pair,
    __torajs_anyv_strict_eq_imm_pair, __torajs_anyv_unbox_tag, __torajs_anyv_unbox_value,
};
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_strict_eq, __torajs_anyv_to_number};
use crate::payload_rc_inc;

/// FFI bridge to NaN-box [`__torajs_anyv_box_from_pair`]. `tag`
/// accepts the same `i64` range as [`crate::AnySlotTag`]
/// discriminants; out-of-range tags fall back to `Null` (defensive —
/// IR shouldn't emit these). The returned `*mut c_void` carries the
/// `AnyValue` bit-pattern, **not** a real heap pointer.
///
/// Heap-tagged values have their refcount bumped here so the
/// boxed AnyValue owns an independent reference (matching the
/// legacy `AnyBox::alloc` semantics ssa_lower's call sites were
/// designed around). The base sister
/// [`__torajs_anyv_box_from_pair`] does **not** rc_inc — its
/// callers (ssa_lower's post-Step-7d Heap path, anyvalue's own
/// inner helpers) transfer ownership explicitly.
///
/// # Safety
///
/// For `tag == AnySlotTag::Heap as i64`, `value` must be either
/// null or a valid `*mut HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_box(tag: i64, value: i64) -> *mut c_void {
    // SAFETY: caller invariant — `payload_rc_inc` is null-safe and
    // dispatches on tag.
    payload_rc_inc(tag, value);
    // SAFETY: caller invariant on (tag, value) propagated.
    let v = unsafe { __torajs_anyv_box_from_pair(tag, value) };
    v as *mut c_void
}

/// FFI bridge — read the boxed payload's tag from a NaN-box
/// `AnyValue` (passed through the legacy `*const c_void` parameter
/// for ABI compatibility with ssa_lower's existing IR).
///
/// # Safety
///
/// `box_ptr` carries an `AnyValue` bit-pattern previously returned
/// by [`__torajs_any_box`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_unbox_tag(box_ptr: *const c_void) -> i64 {
    __torajs_anyv_unbox_tag(box_ptr as AnyValue)
}

/// FFI bridge — decode an `AnyValue`'s raw value field (passed as
/// `*const c_void` for ABI compat).
///
/// # Safety
///
/// `box_ptr` carries an `AnyValue` bit-pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_unbox_value(box_ptr: *const c_void) -> i64 {
    __torajs_anyv_unbox_value(box_ptr as AnyValue)
}

/// FFI bridge to [`payload_rc_inc`]. Bumps the heap child rc
/// for `Heap`-tagged pairs; no-op otherwise. Pair-arg — unchanged
/// from the legacy ABI (operates on the already-decoded tag/value
/// pair so it does not need to read AnyBox struct fields).
///
/// # Safety
///
/// If `tag == Heap`, `value` must be null or a valid `*mut
/// HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_payload_rc_inc(tag: i64, value: i64) {
    payload_rc_inc(tag, value);
}

/// FFI bridge — drop an `AnyValue`. For cell-tagged values
/// (`is_cell`) this rc_dec's the wrapped heap pointer; primitives
/// (int32 / f64 / bool / null / undefined) are no-op. The NaN-box
/// `AnyValue` itself lives in a register / stack slot — there is
/// no heap allocation to free (Step 7d-A: the old `AnyBox::alloc`
/// heap is gone from this code path; Step 7f deletes the struct
/// + alloc helper entirely).
///
/// # Safety
///
/// `box_ptr` is null OR carries an `AnyValue` bit-pattern previously
/// returned by [`__torajs_any_box`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_box_drop(box_ptr: *mut c_void) {
    // SAFETY: AnyValue is just a u64; rc_dec inspects the
    // bit-pattern, only touches heap memory for cell-tagged values.
    unsafe { __torajs_anyv_rc_dec(box_ptr as AnyValue) };
}

/// FFI bridge — Any === Any strict equality (JS spec §7.2.13).
/// Delegates to the NaN-box-immediate strict-eq.
///
/// # Safety
///
/// `l` and `r` each carry an `AnyValue` bit-pattern (or zero, which
/// `__torajs_anyv_strict_eq` treats as `null`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_any_strict_eq(l: *const c_void, r: *const c_void) -> bool {
    // SAFETY: caller invariant propagated.
    unsafe { __torajs_anyv_strict_eq(l as AnyValue, r as AnyValue) }
}

/// FFI bridge to [`any_to_str`]. Pair-arg — unchanged from the
/// legacy ABI (operates on the already-decoded tag/value pair so
/// it does not need to read AnyBox struct fields). Returns a
/// freshly-owned `Str` pointer the caller must drop. Used by
/// ssa_lower at every implicit ToString site (template literals,
/// `+` mixing string and non-string operands, `console.log(any)`
/// printing, …).
///
/// # Safety
///
/// For `tag == Heap`, `value` is null or a valid `*mut
/// HeapHeader` pointing to a live heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_to_str(tag: i64, value: i64) -> *mut c_void {
    unsafe { any_to_str(tag, value) }
}

/// FFI bridge — `ToNumber(Any)` per ES §7.1.4, the Any → numeric
/// coercion sink. Delegates to the NaN-box-immediate ToNumber.
///
/// # Safety
///
/// `box_ptr` carries an `AnyValue` bit-pattern (or zero, which
/// `__torajs_anyv_to_number` treats as `null` → `0.0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_to_number(box_ptr: *const c_void) -> f64 {
    // SAFETY: caller invariant propagated.
    unsafe { __torajs_anyv_to_number(box_ptr as AnyValue) }
}

/// FFI bridge — packed-pair ToNumber. Pair-arg — unchanged from
/// the legacy ABI.
///
/// # Safety
///
/// If `tag == AnySlotTag::Heap as i64`, `value` must be null or
/// a valid `*mut HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_to_number_inner(tag: i64, value: i64) -> f64 {
    // SAFETY: caller invariant propagated.
    unsafe { any_to_number(tag, value) }
}

/// FFI bridge — packed-pair relational compare per ES §7.2.13.
/// Pair-arg — unchanged from the legacy ABI.
///
/// # Safety
///
/// For `lt == AnySlotTag::Heap as i64`, `lv` is null or a valid
/// `*mut HeapHeader`. Same constraint on `(rt, rv)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_compare(op: i64, lt: i64, lv: i64, rt: i64, rv: i64) -> bool {
    // SAFETY: caller invariant — propagated.
    unsafe { any_compare(op, lt, lv, rt, rv) }
}

/// FFI bridge — packed-pair arithmetic dispatch per ES §13.6–§13.9.
/// Delegates to the NaN-box-immediate arith bridge that takes the
/// pair (tag, value) inputs ssa_lower produces and returns an
/// `AnyValue`. The returned `*mut c_void` carries the NaN-box
/// bit-pattern.
///
/// # Safety
///
/// For `lt == AnySlotTag::Heap as i64`, `lv` is null or a valid
/// `*mut HeapHeader`. Same constraint on `(rt, rv)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_arith(
    op: i64,
    lt: i64,
    lv: i64,
    rt: i64,
    rv: i64,
) -> *mut c_void {
    // SAFETY: caller invariant propagated.
    let v = unsafe { __torajs_anyv_arith_pair(op, lt, lv, rt, rv) };
    v as *mut c_void
}

/// FFI bridge — packed-pair `+` per ES §13.15.3. Delegates to the
/// NaN-box-immediate add bridge. Returns an `AnyValue` bit-pattern
/// in the `*mut c_void` slot.
///
/// # Safety
///
/// For `lt == AnySlotTag::Heap as i64`, `lv` is null or a valid
/// `*mut HeapHeader`. Same constraint on `(rt, rv)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_add(lt: i64, lv: i64, rt: i64, rv: i64) -> *mut c_void {
    // SAFETY: caller invariant propagated.
    let v = unsafe { __torajs_anyv_add_pair(lt, lv, rt, rv) };
    v as *mut c_void
}

/// FFI bridge — Any === concrete (SSA-emitted `(tag, value)` pair
/// vs an `AnyValue`). Avoids a fresh box alloc per compare site.
/// Delegates to the immediate-pair strict-eq sister.
///
/// # Safety
///
/// `box_ptr` carries an `AnyValue` bit-pattern (or zero). `rhs_tag`
/// is a well-formed [`crate::AnySlotTag`] discriminant; `rhs_value`
/// is the packing the SSA layer chose (bitcast for f64, zext for
/// bool, raw cast for i64, pointer-as-i64 for heap).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_strict_eq(
    box_ptr: *const c_void,
    rhs_tag: i64,
    rhs_value: i64,
) -> bool {
    // SAFETY: caller invariant propagated.
    unsafe { __torajs_anyv_strict_eq_imm_pair(box_ptr as AnyValue, rhs_tag, rhs_value) }
}
