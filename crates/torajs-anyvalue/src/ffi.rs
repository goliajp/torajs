//! `__torajs_any_*` extern "C" FFI shims — thin wrappers around the
//! internal Rust impls, called from ssa_lower-emitted IR at every
//! Type::Any source site.
//!
//! Every fn here is `#[unsafe(no_mangle)]` + `pub unsafe extern "C"`
//! so the staticlib's symbol table exports the C-ABI name verbatim.
//! Internal helpers (`any_to_str`, `any_to_number`, `any_compare`,
//! `any_arith`, `any_add`, `payload_rc_inc`, `payload_eq`) live in
//! `lib.rs` and are exposed via `pub(crate)` so this module can
//! call them without leaking them to downstream crates.
//!
//! Extracted from `lib.rs` (2026-05-25, anyvalue god-file decomp).

use std::ffi::c_void;
use std::ptr::NonNull;

use crate::arith::{any_add, any_arith};
use crate::compare::any_compare;
use crate::{AnyBox, AnySlotTag, any_to_number, any_to_str, payload_eq, payload_rc_inc};

/// FFI bridge to [`AnyBox::alloc`]. `tag` accepts the same `i64`
/// range as [`AnySlotTag`] discriminants; out-of-range tags fall
/// back to `Null` (defensive — IR shouldn't emit these).
///
/// # Safety
///
/// For `tag == AnySlotTag::Heap as i64`, `value` must be either
/// null or a valid `*mut HeapHeader` (the new box gains an owning
/// ref via `rc_inc`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_box(tag: i64, value: i64) -> *mut c_void {
    let slot = match tag {
        0 => AnySlotTag::Null,
        1 => AnySlotTag::Bool,
        2 => AnySlotTag::I64,
        3 => AnySlotTag::F64,
        4 => AnySlotTag::Heap,
        5 => AnySlotTag::Undef,
        _ => AnySlotTag::Null,
    };
    AnyBox::alloc(slot, value).as_ptr() as *mut c_void
}

/// FFI bridge — read the boxed payload's tag.
///
/// # Safety
///
/// `box_ptr` must be a valid `*const AnyBox` (i.e. previously
/// returned by [`__torajs_any_box`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_unbox_tag(box_ptr: *const c_void) -> i64 {
    // SAFETY: caller invariant.
    unsafe { (*(box_ptr as *const AnyBox)).tag }
}

/// FFI bridge — read the boxed payload's raw value.
///
/// # Safety
///
/// `box_ptr` must be a valid `*const AnyBox`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_unbox_value(box_ptr: *const c_void) -> i64 {
    // SAFETY: caller invariant.
    unsafe { (*(box_ptr as *const AnyBox)).value }
}

/// FFI bridge to [`payload_rc_inc`]. Bumps the heap child rc
/// for `Heap`-tagged pairs; no-op otherwise.
///
/// # Safety
///
/// If `tag == Heap`, `value` must be null or a valid `*mut
/// HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_payload_rc_inc(tag: i64, value: i64) {
    payload_rc_inc(tag, value);
}

/// FFI bridge to [`AnyBox::drop_owned`]. Null-safe.
///
/// # Safety
///
/// `box_ptr` is null OR a valid `*mut AnyBox` previously returned
/// by [`__torajs_any_box`]; caller exclusively owns it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_box_drop(box_ptr: *mut c_void) {
    if let Some(p) = NonNull::new(box_ptr as *mut AnyBox) {
        // SAFETY: caller invariant.
        unsafe { AnyBox::drop_owned(p) };
    }
}

/// FFI bridge — Any === Any strict equality (JS spec §7.2.13).
///
/// # Safety
///
/// `l` and `r` are each null OR a valid `*const AnyBox`. Two-null
/// is true, one-null is false.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_any_strict_eq(l: *const c_void, r: *const c_void) -> bool {
    match (l.is_null(), r.is_null()) {
        (true, true) => true,
        (true, _) | (_, true) => false,
        _ => {
            // SAFETY: both ptrs non-null per the match arm.
            let lb = unsafe { &*(l as *const AnyBox) };
            let rb = unsafe { &*(r as *const AnyBox) };
            if lb.tag != rb.tag {
                return false;
            }
            payload_eq(lb.tag, lb.value, rb.value)
        }
    }
}

/// FFI bridge to [`any_to_str`]. Returns a freshly-owned `Str`
/// pointer the caller must drop. Used by ssa_lower at every
/// implicit ToString site (template literals, `+` mixing string
/// and non-string operands, `console.log(any)` printing, …).
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
/// coercion sink. Public symbol declared by ssa_lower's
/// `coerce_any_to_number` (used at every `return <any>` whose
/// declared return is a concrete number, every `+` between number
/// and Any, etc.).
///
/// Null box is defensive (a real Any always carries a box);
/// `ToNumber(null)` is `0` per spec, matched here.
///
/// # Safety
///
/// `box_ptr` is null OR a valid `*const AnyBox` previously
/// returned by [`__torajs_any_box`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_to_number(box_ptr: *const c_void) -> f64 {
    if box_ptr.is_null() {
        return 0.0;
    }
    // SAFETY: non-null per the early return.
    let b = unsafe { &*(box_ptr as *const AnyBox) };
    // SAFETY: well-formed AnyBox — if tag is Heap then value is
    // either null or a valid *mut HeapHeader.
    unsafe { any_to_number(b.tag, b.value) }
}

/// FFI bridge — packed-pair ToNumber. Currently used by the
/// in-file C callers (`__torajs_any_compare`, `__torajs_any_arith`)
/// in `runtime_str.c` that haven't been ported yet (P2.3-d.2 and
/// .3). Once those move to Rust those callers vanish, but the
/// shim stays public for any future packed-pair callsite.
///
/// # Safety
///
/// If `tag == AnySlotTag::Heap as i64`, `value` must be null or
/// a valid `*mut HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_to_number_inner(tag: i64, value: i64) -> f64 {
    // SAFETY: caller invariant — propagated.
    unsafe { any_to_number(tag, value) }
}

/// FFI bridge — packed-pair relational compare per ES §7.2.13.
/// `op` is 0=Lt, 1=Le, 2=Gt, 3=Ge; out-of-range op codes return
/// `false` defensively (IR should never emit them). Used by
/// ssa_lower at every `<` / `<=` / `>` / `>=` site where either
/// operand is Any-typed.
///
/// # Safety
///
/// For `lt == AnySlotTag::Heap as i64`, `lv` is null or a valid
/// `*mut HeapHeader`. Same constraint on `(rt, rv)`. Caller
/// promises tags are well-formed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_compare(op: i64, lt: i64, lv: i64, rt: i64, rv: i64) -> bool {
    // SAFETY: caller invariant — propagated.
    unsafe { any_compare(op, lt, lv, rt, rv) }
}

/// FFI bridge — packed-pair arithmetic dispatch per ES §13.6–§13.9.
/// `op` is 0=Sub, 1=Mul, 2=Div, 3=Mod; out-of-range op codes return
/// a NaN-boxed AnyBox defensively. Used by ssa_lower at every
/// `-` / `*` / `/` / `%` site where either operand is Any-typed.
/// Returns a fresh owned AnyBox (refcount = 1); caller must drop.
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
    // SAFETY: caller invariant — propagated.
    unsafe { any_arith(op, lt, lv, rt, rv) }
}

/// FFI bridge — packed-pair `+` per ES §13.15.3. Used by ssa_lower
/// at every `+` site where either operand is Any-typed. Returns a
/// fresh owned AnyBox (Heap-tagged Str for the concat path, I64 or
/// F64 for the numeric path); caller must drop.
///
/// # Safety
///
/// For `lt == AnySlotTag::Heap as i64`, `lv` is null or a valid
/// `*mut HeapHeader`. Same constraint on `(rt, rv)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_add(lt: i64, lv: i64, rt: i64, rv: i64) -> *mut c_void {
    // SAFETY: caller invariant — propagated.
    unsafe { any_add(lt, lv, rt, rv) }
}

/// FFI bridge — Any === concrete (SSA-emitted `(tag, value)` pair
/// vs a box). Avoids a fresh box alloc per compare site.
///
/// `box_ptr == null` matches `rhs_tag == AnySlotTag::Null` and
/// nothing else.
///
/// # Safety
///
/// `box_ptr` is null OR a valid `*const AnyBox`. `rhs_tag` is a
/// well-formed [`AnySlotTag`] discriminant; `rhs_value` is the
/// packing the SSA layer chose (bitcast for f64, zext for bool,
/// raw cast for i64, pointer-as-i64 for heap).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_strict_eq(
    box_ptr: *const c_void,
    rhs_tag: i64,
    rhs_value: i64,
) -> bool {
    if box_ptr.is_null() {
        return rhs_tag == AnySlotTag::Null as i64;
    }
    // SAFETY: non-null per the early return.
    let b = unsafe { &*(box_ptr as *const AnyBox) };
    if b.tag != rhs_tag {
        return false;
    }
    payload_eq(b.tag, b.value, rhs_value)
}
