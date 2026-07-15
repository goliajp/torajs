//! RFC 20260716-primitive-wrapper-substrate 刀 3 — primitive-wrapper
//! view-through decode.
//!
//! A `new Number(x)` / `new String(s)` / `new Boolean(b)` cell answers
//! its instance-method surface (ES §21.1 / §21.4 / §22.1) by reading
//! `[[NumberData]]` / `[[StringData]]` / `[[BooleanData]]` and running
//! the wrapped primitive's kernel. Rather than duplicating every str /
//! num / bool method arm for wrapper receivers, the four Any-dispatch
//! entry points (`__torajs_any_method_call` / `_length_get` /
//! `_index_get`) decode the inner payload here and re-dispatch through
//! the primitive lane — one place decides the mapping, every existing
//! arm handles it.
//!
//! Ownership: the caller's +1 stays on the wrapper cell across the
//! delegated call. The primitive lane's return mints its own +1 per
//! the boxed-value convention, so no bookkeeping bleeds across.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::{
    AnyValue, VALUE_FALSE, VALUE_TRUE, box_double, box_void_ptr, try_box_short_str,
};

/// The inner-primitive `AnyValue` a wrapper cell delegates to, or
/// `None` for a non-wrapper tag (the caller keeps its per-tag arm).
///
/// # Safety
/// `ptr` is a live heap cell whose header tag is `tag`.
pub(crate) unsafe fn resolve_inner_recv(ptr: *mut c_void, tag: u16) -> Option<AnyValue> {
    if tag == Tag::NumberWrapper as u16 {
        let num = unsafe { (ptr.cast::<u8>().add(8) as *const f64).read() };
        return Some(box_double(num));
    }
    if tag == Tag::BooleanWrapper as u16 {
        let b = unsafe { (ptr.cast::<u8>().add(8) as *const u8).read() };
        return Some(if b != 0 { VALUE_TRUE } else { VALUE_FALSE });
    }
    if tag == Tag::StringWrapper as u16 {
        let inner_ptr = unsafe { (ptr.cast::<u8>().add(8) as *const *mut c_void).read() };
        return Some(if inner_ptr.is_null() {
            // `new String(NULL)` sentinel → "" — an empty ShortStr
            // carries the same method surface as an empty Str cell.
            try_box_short_str(b"").unwrap()
        } else {
            box_void_ptr(inner_ptr)
        });
    }
    None
}
