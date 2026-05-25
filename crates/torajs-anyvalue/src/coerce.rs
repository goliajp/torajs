//! `ToString` (ES §7.1.17) and `ToNumber` (ES §7.1.4) coercions for
//! Any-tagged operands.
//!
//! Two `pub(crate)` entry points wrapped by ffi.rs:
//!
//! - [`any_to_str`] — tag-dispatch ToString. Returns a freshly-owned
//!   `*mut Str` the caller drops. ffi.rs wraps as
//!   `__torajs_any_to_str`.
//! - [`any_to_number`] — tag-dispatch ToNumber. Returns `f64`. ffi.rs
//!   wraps as `__torajs_any_to_number_inner` (and box-aware
//!   `__torajs_any_to_number`).
//!
//! Plus `AnyValue::to_number()` — idiomatic-Rust mirror for the
//! already-materialized AnyValue view.
//!
//! Extracted from `lib.rs` (2026-05-25, anyvalue god-file decomp
//! batch 14).

use std::ffi::c_void;

use torajs_rc::{__torajs_rc_inc, AnySlotTag, HeapHeader, Tag};

use crate::{
    __torajs_bool_to_str, __torajs_f64_to_str, __torajs_i64_to_str, __torajs_null_to_str,
    __torajs_str_alloc_pooled, __torajs_str_to_number, __torajs_undefined_to_str, AnyValue,
    STR_HDR_SIZE,
};

/// Tag-dispatch ToString for a packed `(tag, value)` pair.
/// Always returns a freshly owned `*mut Str` (refcount = 1)
/// that the caller is responsible for dropping.
///
/// - `Null` → `"null"` (via [`__torajs_null_to_str`])
/// - `Undef` → `"undefined"` (per ES §7.1.17.1)
/// - `Bool` → `"true"` / `"false"`
/// - `I64` → decimal print
/// - `F64` → IEEE pretty-print (`f64::to_string`-like via the C
///   helper, kept C through Layer-2 so format semantics stay
///   identical to bun's `console.log`)
/// - `Heap` + `Tag::Str` → rc_inc the same Str pointer + return
///   (no new alloc; caller owns one ref)
/// - `Heap` + other tag → `"[object]"` placeholder until P3 lands
///   per-type pretty-print
///
/// # Safety
///
/// If `tag == Heap`, `value` must be null or a valid `*mut
/// HeapHeader`. The returned pointer is valid until the caller
/// `drop`s it (matches the pre-rewrite C contract).
pub(crate) unsafe fn any_to_str(tag: i64, value: i64) -> *mut c_void {
    if tag == AnySlotTag::Null as i64 {
        return unsafe { __torajs_null_to_str() };
    }
    if tag == AnySlotTag::Undef as i64 {
        return unsafe { __torajs_undefined_to_str() };
    }
    if tag == AnySlotTag::Bool as i64 {
        return unsafe { __torajs_bool_to_str((value != 0) as i32) };
    }
    if tag == AnySlotTag::I64 as i64 {
        return unsafe { __torajs_i64_to_str(value) };
    }
    if tag == AnySlotTag::F64 as i64 {
        return unsafe { __torajs_f64_to_str(f64::from_bits(value as u64)) };
    }
    if tag == AnySlotTag::Heap as i64 {
        let child = value as *mut HeapHeader;
        if child.is_null() {
            return unsafe { __torajs_null_to_str() };
        }
        // SAFETY: child is non-null per the check above; runtime
        // invariant says it points to a valid header.
        let h = unsafe { &*child };
        if matches!(h.tag(), Tag::Str) {
            // Tag::Str case: just rc_inc + return; the caller now
            // owns one (additional) reference.
            unsafe { __torajs_rc_inc(child as *mut c_void) };
            return child as *mut c_void;
        }
        // Object placeholder. Replaced by per-type pretty-print
        // when P3 lands proper ToString dispatch.
        const PLACEHOLDER: &[u8] = b"[object]";
        // SAFETY: str_alloc_pooled returns a Str-shaped heap with
        // header + len fields written; the body slot starts at
        // `STR_HDR_SIZE` and is `len` bytes wide. We write
        // exactly 8 bytes there.
        unsafe {
            let p = __torajs_str_alloc_pooled(PLACEHOLDER.len() as u64);
            core::ptr::copy_nonoverlapping(
                PLACEHOLDER.as_ptr(),
                p.add(STR_HDR_SIZE),
                PLACEHOLDER.len(),
            );
            p as *mut c_void
        }
    } else {
        // Unknown tag (defensive): treat as null.
        unsafe { __torajs_null_to_str() }
    }
}

/// Tag-dispatch `ToNumber` for a packed `(tag, value)` pair —
/// mirrors ES §7.1.4 over tora's tagged-Any subset:
///
/// - `Null` → `0.0`
/// - `Undef` → `NaN`
/// - `Bool` → `1.0` / `0.0`
/// - `I64` → cast to `f64`
/// - `F64` → bitcast `i64`-bits → `f64`
/// - `Heap` + null pointer → `0.0` (defensive — `Heap`-tag NULL
///   doesn't carry numeric semantics; the C ABI returned 0 here)
/// - `Heap` + [`Tag::Str`] → parse via [`__torajs_str_to_number`]
/// - `Heap` + other tag → `NaN` (objects coerce to NaN until the
///   `valueOf` method dispatch lands in a later phase)
/// - unknown tag → `NaN` (defensive)
///
/// # Safety
///
/// If `tag == AnySlotTag::Heap as i64`, `value` must be either
/// null or a valid `*const HeapHeader` pointing to a live heap
/// object.
pub(crate) unsafe fn any_to_number(tag: i64, value: i64) -> f64 {
    if tag == AnySlotTag::Null as i64 {
        return 0.0;
    }
    if tag == AnySlotTag::Undef as i64 {
        return f64::NAN;
    }
    if tag == AnySlotTag::Bool as i64 {
        return if value != 0 { 1.0 } else { 0.0 };
    }
    if tag == AnySlotTag::I64 as i64 {
        return value as f64;
    }
    if tag == AnySlotTag::F64 as i64 {
        return f64::from_bits(value as u64);
    }
    if tag == AnySlotTag::Heap as i64 {
        let child = value as *const HeapHeader;
        if child.is_null() {
            return 0.0;
        }
        // SAFETY: child non-null per the check above; runtime
        // invariant says it points to a live heap header.
        let h = unsafe { &*child };
        if matches!(h.tag(), Tag::Str) {
            // SAFETY: child is Tag::Str-headed; the C-side
            // __torajs_str_to_number reads the Str layout from
            // the header.
            return unsafe { __torajs_str_to_number(child as *const c_void) };
        }
        return f64::NAN;
    }
    f64::NAN
}

impl AnyValue {
    /// `ToNumber` per ES §7.1.4 — idiomatic-Rust mirror of
    /// [`any_to_number`] for already-materialized `AnyValue`s.
    /// Same per-tag rules; the `Heap` case delegates to the C
    /// `__torajs_str_to_number` for `Tag::Str` and returns `NaN`
    /// for every other heap type.
    pub fn to_number(self) -> f64 {
        match self {
            AnyValue::Null => 0.0,
            AnyValue::Undef => f64::NAN,
            AnyValue::Bool(b) => {
                if b {
                    1.0
                } else {
                    0.0
                }
            }
            AnyValue::I64(n) => n as f64,
            AnyValue::F64(n) => n,
            AnyValue::Heap(None) => 0.0,
            AnyValue::Heap(Some(p)) => {
                // SAFETY: NonNull invariant — points to a live
                // HeapHeader.
                let h = unsafe { p.as_ref() };
                if matches!(h.tag(), Tag::Str) {
                    // SAFETY: pointer is Tag::Str-headed.
                    unsafe { __torajs_str_to_number(p.as_ptr() as *const c_void) }
                } else {
                    f64::NAN
                }
            }
            AnyValue::Unknown => f64::NAN,
        }
    }
}
