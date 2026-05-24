//! Any-tag inspection — `typeof box` + `console.log(box)` —
//! port of `runtime_str.c` L1040-1157.
//!
//! Two extern fns that read an [`AnyBox`]'s discriminant and route:
//!
//! - [`__torajs_any_typeof`] — returns a fresh Str holding the ES
//!   `typeof` result for the box (`"object"` / `"undefined"` /
//!   `"boolean"` / `"number"` / `"string"` / `"function"` /
//!   `"symbol"` / `"bigint"`).
//!
//! - [`__torajs_print_any`] — `console.log(box)` dispatch. Routes to
//!   the IR-emitted `print_i64` / `print_f64` / `print_bool` and
//!   `__torajs_str_print` based on the slot tag. ANY_HEAP recurses
//!   through the heap value's universal `type_tag`; only `Str` gets
//!   pretty-printed today, everything else falls back to
//!   `"[object]\n"` (heap-typed pretty-print is a later wedge).
//!
//! Cross-tier symbols resolved at `tr build` link time:
//! - `print_i64` / `print_f64` / `print_bool` — IR-emitted in
//!   ssa_inkwell (per-byte putchar stdio buffer).
//! - `__torajs_str_print` — `libtorajs_str.a`.
//! - `__torajs_str_alloc_pooled` — `libtorajs_str.a`.
//! - `putchar` — libc; per-byte writer shared with the print family.

use core::ffi::c_void;

use crate::AnyBox;
use torajs_rc::{AnySlotTag, HeapHeader, Tag};

const STR_HDR_SIZE: usize = 16;

unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_print(s: *const u8);
    fn print_i64(n: i64);
    fn print_f64(d: f64);
    fn print_bool(b: bool);
    fn putchar(c: i32) -> i32;
}

#[inline]
fn alloc_literal(s: &[u8]) -> *mut u8 {
    let p = unsafe { __torajs_str_alloc_pooled(s.len() as u64) };
    if !s.is_empty() {
        unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), p.add(STR_HDR_SIZE), s.len()) };
    }
    p
}

#[inline]
fn write_line(s: &[u8]) {
    for &b in s {
        unsafe { putchar(b as i32) };
    }
}

#[inline]
unsafe fn heap_type_tag(child: *const c_void) -> u16 {
    unsafe { (*(child as *const HeapHeader)).type_tag }
}

/// `typeof box` per ES §13.5.3 — returns a fresh Str. NULL box
/// (uninit / explicit cast) treats as `"object"` per spec
/// (`typeof null === "object"`).
///
/// # Safety
/// `box_ptr` must be NULL or a valid `*const AnyBox` (live).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_typeof(box_ptr: *const c_void) -> *mut u8 {
    if box_ptr.is_null() {
        return alloc_literal(b"object");
    }
    let any = unsafe { &*(box_ptr as *const AnyBox) };
    let kind: &[u8] = match any.slot_tag() {
        Some(AnySlotTag::Null) => b"object",
        Some(AnySlotTag::Undef) => b"undefined",
        Some(AnySlotTag::Bool) => b"boolean",
        Some(AnySlotTag::I64) | Some(AnySlotTag::F64) => b"number",
        Some(AnySlotTag::Heap) => {
            let child = any.value as *const c_void;
            if child.is_null() {
                b"object"
            } else {
                // SAFETY: ANY_HEAP value is a refcounted heap ptr; we
                // only read the universal type_tag at offset +4 in the
                // header.
                let tag = unsafe { heap_type_tag(child) };
                if tag == Tag::Str as u16 {
                    b"string"
                } else if tag == Tag::Closure as u16 {
                    b"function"
                } else if tag == Tag::Symbol as u16 {
                    b"symbol"
                } else if tag == Tag::BigInt as u16 {
                    b"bigint"
                } else {
                    // OBJ / ARR / REGEX / DATE / RESPONSE / WEAK* /
                    // ANY_BOX (nested) / DYNOBJ / MAP* / ARR_ITER →
                    // "object"
                    b"object"
                }
            }
        }
        None => b"object",
    };
    alloc_literal(kind)
}

/// `console.log(box)` dispatch — single-arg form. Routes to the
/// matching primitive printer based on the slot tag; ANY_HEAP
/// recurses through the heap value's universal `type_tag` for
/// `Str` (the only pretty-printable heap type today).
///
/// Trailing newline matches every other `print_*` helper; multi-
/// arg console.log goes through the space-joiner upstream and
/// calls this for each arg in turn.
///
/// # Safety
/// `box_ptr` must be NULL or a valid `*const AnyBox` (live).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_any(box_ptr: *const c_void) {
    if box_ptr.is_null() {
        write_line(b"null\n");
        return;
    }
    let any = unsafe { &*(box_ptr as *const AnyBox) };
    match any.slot_tag() {
        Some(AnySlotTag::Null) => write_line(b"null\n"),
        Some(AnySlotTag::Undef) => write_line(b"undefined\n"),
        Some(AnySlotTag::Bool) => unsafe { print_bool(any.value != 0) },
        Some(AnySlotTag::I64) => unsafe { print_i64(any.value) },
        Some(AnySlotTag::F64) => {
            // i64 → f64 bitcast.
            let d = f64::from_bits(any.value as u64);
            unsafe { print_f64(d) };
        }
        Some(AnySlotTag::Heap) => {
            let child = any.value as *const c_void;
            if child.is_null() {
                write_line(b"null\n");
                return;
            }
            // SAFETY: live heap ptr; reading type_tag is the same
            // pattern as `__torajs_any_typeof`.
            let tag = unsafe { heap_type_tag(child) };
            if tag == Tag::Str as u16 {
                unsafe { __torajs_str_print(child as *const u8) };
            } else {
                // Obj / Arr / Closure / RegExp / Date pretty-print
                // is a later wedge. For now print a placeholder so
                // the user sees something rather than silent / crash.
                write_line(b"[object]\n");
            }
        }
        None => write_line(b"[unknown-any-tag]\n"),
    }
}
