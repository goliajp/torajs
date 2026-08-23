//! `ToBigInt` (ES §7.1.13) over a NaN-boxed Any operand.
//!
//! Serves the ns-static `BigInt.asIntN` / `asUintN` dispatch arms
//! (RFC 20260720 刀 5b-2) — the typed lowering never needs this
//! (its args are statically BigInt), but a reified-cell call sees
//! arbitrary Any args.
//!
//! Spec dispatch:
//!
//! - BigInt      → the value itself (fresh stake via rc_inc)
//! - Boolean     → `0n` / `1n`
//! - String      → §7.1.14 StringToBigInt; reject → SyntaxError
//! - Number      → TypeError (never implicit-converts)
//! - undefined / null / Symbol → TypeError
//! - Object      → ToPrimitive(hint number), then the primitive
//!   re-enters the dispatch (one level — ToPrimitive never answers
//!   an object).

use torajs_rc::{HeapHeader, Tag};

use crate::loose_eq::bigint_ffi;
use crate::nanbox::{AnyValue, as_void_ptr, is_bool, is_cell, is_short_str, is_true};

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_syntax_error(msg: *const core::ffi::c_char);
}

#[cfg(test)]
unsafe fn __torajs_throw_type_error(_msg: *const core::ffi::c_char) {}
#[cfg(test)]
unsafe fn __torajs_throw_syntax_error(_msg: *const core::ffi::c_char) {}

/// §7.1.13 — coerce `v` to an owned BigInt heap pointer (rc stake
/// belongs to the caller). `None` = a pending throw was recorded
/// (TypeError / SyntaxError / a poisoned valueOf during
/// ToPrimitive); the caller unwinds.
///
/// # Safety
/// `v` is a live NaN-boxed Any value.
pub(crate) unsafe fn any_to_bigint(v: AnyValue) -> Option<*mut u8> {
    unsafe {
        if is_bool(v) {
            return Some(bigint_ffi::__torajs_bigint_from_number(if is_true(v) {
                1.0
            } else {
                0.0
            }));
        }
        if is_short_str(v) || (is_cell(v) && matches!(cell_tag(v), Tag::Str)) {
            // String lane — materialize (owned Str cell), parse
            // strict, release the temp.
            let s = crate::nanbox_ffi::__torajs_anyv_to_str(v);
            let b = bigint_ffi::__torajs_bigint_from_str_strict(s);
            crate::__torajs_str_drop(s);
            if b.is_null() {
                __torajs_throw_syntax_error(c"Failed to parse String to BigInt".as_ptr());
                return None;
            }
            return Some(b);
        }
        if is_cell(v) {
            match cell_tag(v) {
                Tag::BigInt => {
                    crate::nanbox_ffi::__torajs_anyv_rc_inc(v);
                    return Some(as_void_ptr(v) as *mut u8);
                }
                Tag::Symbol => {
                    __torajs_throw_type_error(
                        c"Invalid argument type in ToBigInt operation".as_ptr(),
                    );
                    return None;
                }
                _ => {
                    // Object lane — ToPrimitive(hint number), then the
                    // primitive re-enters (ToPrimitive never answers
                    // an object; a Str/BigInt/Bool result converts, a
                    // Number/nullish/Symbol result rejects below).
                    let prim = crate::to_primitive::heap_to_primitive(as_void_ptr(v), false)?;
                    let r = any_to_bigint(prim);
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(prim);
                    return r;
                }
            }
        }
        // Number (int32 / double), undefined, null — TypeError.
        __torajs_throw_type_error(c"Invalid argument type in ToBigInt operation".as_ptr());
        None
    }
}

/// §21.2.1.1 `BigInt(value)` over an Any operand — ToPrimitive
/// (hint number) FIRST, then a Number primitive takes NumberToBigInt
/// (integral or RangeError) where plain ToBigInt would TypeError;
/// every other primitive re-enters the §7.1.13 dispatch. NULL = a
/// pending throw was recorded.
///
/// # Safety
/// `v` is a live NaN-boxed Any value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_ctor_any(v: AnyValue) -> *mut u8 {
    unsafe {
        let (prim, owned) =
            if is_cell(v) && !matches!(cell_tag(v), Tag::BigInt | Tag::Symbol | Tag::Str) {
                match crate::to_primitive::heap_to_primitive(as_void_ptr(v), false) {
                    Some(p) => (p, true),
                    None => return core::ptr::null_mut(),
                }
            } else {
                (v, false)
            };
        let r = if crate::nanbox::is_int32(prim) {
            bigint_ffi::__torajs_bigint_from_number(crate::nanbox::as_int32(prim) as f64)
        } else if crate::nanbox::is_double(prim) {
            // NumberToBigInt — from_number's own RangeError gate
            // covers the non-integral / non-finite cases.
            bigint_ffi::__torajs_bigint_from_number(crate::nanbox::as_double(prim))
        } else {
            any_to_bigint(prim).unwrap_or(core::ptr::null_mut())
        };
        if owned {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(prim);
        }
        r
    }
}

/// Heap tag of a cell-form Any — caller already gated on `is_cell`.
#[inline]
unsafe fn cell_tag(v: AnyValue) -> Tag {
    let h = as_void_ptr(v) as *const HeapHeader;
    unsafe { (*h).tag() }
}

/// C-ABI face for the SSA coerce lanes (`coerce_any_to_bigint` —
/// the checker's Any→BigInt call-boundary admit pairs with this).
/// NULL = a pending throw was recorded; the emit site's throw check
/// unwinds before the value is ever used.
///
/// # Safety
/// `v` is a live NaN-boxed Any value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_to_bigint(v: u64) -> *mut u8 {
    unsafe { any_to_bigint(v).unwrap_or(core::ptr::null_mut()) }
}
