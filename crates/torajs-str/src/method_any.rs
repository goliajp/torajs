//! `any`-receiver String method glue (Any-method-call RFC 20260704
//! C1) — `s.charAt(i)` / `s.toUpperCase()` / `s.toLowerCase()` where
//! `s` crossed into the `any` world.
//!
//! Called by `__torajs_any_method_call`'s Str arm (torajs-anyvalue)
//! with an already-unboxed heap Str/Substr pointer. Returns raw
//! AnyValue bits — a heap cell's NaN-box encoding IS its pointer
//! bits, so a fresh rc=1 Str/Substr returns as `ptr as u64`
//! (ownership transfers to the caller, matching the boxed-value
//! convention).

use torajs_rc::HeapHeader;

use crate::block::__torajs_str_alloc;
use crate::index_any::__torajs_str_index_get;
use crate::str_drop::__torajs_str_drop;
use crate::substr::{FLAG_SUBSTR_INLINE, FLAG_SUBSTR_VIEW};
use crate::substr_methods::__torajs_substr_to_owned;
use crate::transform::case::{__torajs_str_to_lower, __torajs_str_to_upper};

/// `s.charAt(idx)` per ES §22.1.3.1 — one-code-unit string, or the
/// EMPTY string for out-of-range (unlike `s[idx]`'s `undefined`).
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_char_at(s: *mut u8, idx: i64) -> u64 {
    unsafe {
        let out = __torajs_str_index_get(s, idx);
        if out.is_null() {
            // OOB: charAt answers "" (a fresh rc=1 empty Str).
            return __torajs_str_alloc(core::ptr::null(), 0) as u64;
        }
        out as u64
    }
}

/// `s.toUpperCase()` / `s.toLowerCase()` per ES §22.1.3.29/30 —
/// `upper != 0` picks the uppercase tables. Substr views (same
/// `Tag::Str`, parent+offset layout the case tables' inline
/// `str_view` can't read) materialize to an owned temp first.
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_case(s: *const u8, upper: i64) -> u64 {
    unsafe {
        let header = &*(s as *const HeapHeader);
        let owned_tmp = if header.flags & (FLAG_SUBSTR_INLINE | FLAG_SUBSTR_VIEW) != 0 {
            __torajs_substr_to_owned(s) as *mut u8
        } else {
            core::ptr::null_mut()
        };
        let src = if owned_tmp.is_null() { s } else { owned_tmp };
        let out = if upper != 0 {
            __torajs_str_to_upper(src)
        } else {
            __torajs_str_to_lower(src)
        };
        if !owned_tmp.is_null() {
            __torajs_str_drop(owned_tmp);
        }
        out as u64
    }
}
