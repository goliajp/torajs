//! `__torajs_str_index_get` — `s[idx]` for a string reached through
//! an `any` receiver (Any-dynamic-access RFC 20260704 S3).
//!
//! Unlike `charAt` (OOB → empty string, ES §22.1.3.1), bare index
//! access follows the ordinary property lookup path: out-of-bounds /
//! negative index → no such property → the caller boxes `undefined`.
//! This fn signals that case with NULL; in-bounds returns a fresh
//! 1-code-unit `Substr` view (rc=1, ownership transfers to the
//! caller).
//!
//! Handles both heap string shapes behind one entry — the dispatch
//! site only knows "Tag::Str cell": plain `Str` routes through
//! [`crate::transform::__torajs_str_char_at`], `Substr` views
//! (FLAG_SUBSTR_INLINE) through
//! [`crate::substr_methods::__torajs_substr_slice`] so the fresh
//! view chains to the same root parent.

use crate::str_drop::__torajs_str_drop;
use crate::substr::{FLAG_SUBSTR_INLINE, SUBSTR_LEN_OFF};
use crate::substr_methods_slice::__torajs_substr_slice;
use crate::transform::construct::__torajs_str_char_at;

/// See module doc. NULL / negative / OOB → NULL (caller maps to
/// `undefined`); in-bounds → fresh 1-unit Substr (caller owns).
///
/// # Safety
/// `s` is either NULL or a valid `Tag::Str`-tagged heap block
/// (plain Str or Substr view).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_index_get(s: *mut u8, idx: i64) -> *mut u8 {
    if s.is_null() || idx < 0 {
        return core::ptr::null_mut();
    }
    unsafe {
        let flags = *(s.add(6) as *const u16);
        let sub = if flags & FLAG_SUBSTR_INLINE != 0 {
            __torajs_substr_slice(s, idx, idx + 1) as *mut u8
        } else {
            __torajs_str_char_at(s, idx)
        };
        // Both producers clamp OOB to a length-0 view; a legal
        // single-unit read is always length 1, so length 0 ⟺ OOB.
        let len = *(sub.add(SUBSTR_LEN_OFF) as *const u64);
        if len == 0 {
            __torajs_str_drop(sub);
            return core::ptr::null_mut();
        }
        sub
    }
}
