//! Which builtin lane would claim a receiver — the side-effect-free
//! mirror of [`crate::iter_any::iter_next_inner`]'s cascade.
//!
//! Split out of `iter_any.rs` at the 500-line cap. The parent STEPS a
//! receiver; this file answers the prior question `Array.from` has to
//! ask before it mints anything: would stepping claim this value at
//! all, or is the array-like walk what is left of it.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, as_void_ptr, is_cell, is_short_str};

use torajs_rc::Tag;

use crate::iter_any::{SYM_ASYNC_ITERATOR_METHOD, SYM_ITERATOR_METHOD};

unsafe extern "C" {
    /// torajs-structmeta — read side over `__torajs_class_layouts`
    /// (NULL for class_tag 0 / past the table).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    /// torajs-structmeta — class-method boxed adapter by name bytes
    /// (NULL miss).
    fn __torajs_struct_method_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
    ) -> *const c_void;
}

/// Side-effect-free mirror of the cascade's builtin claims — `true`
/// when a receiver that answered `NoUserMethod` to the `@@iterator`
/// probe would still be claimed by a builtin lane below (string /
/// array / wrapper / Map / Set / iterator cells / a class instance
/// with a `[Symbol.iterator]()` member). `Array.from`'s construct
/// face (§23.1.2.1 step 4 usingIterator) needs the verdict WITHOUT
/// stepping, because the two sides mint A differently
/// (`Construct(C)` vs `Construct(C, «len»)`).
///
/// Keep in lockstep with [`iter_next_inner`]'s lane order — a lane
/// added there without a row here silently sends the new shape down
/// the array-like walk.
///
/// # Safety
/// `recv` is a live AnyValue.
pub(crate) unsafe fn claims_iterable(recv: AnyValue) -> bool {
    // A family prototype whose `[Symbol.iterator]` was deleted no
    // longer claims its receivers — the array-like walk is what is
    // left of them (§23.1.2.1 step 4).
    if unsafe { crate::iter_any_get_method::builtin_iterator_missing(recv) } {
        return false;
    }
    if is_short_str(recv) {
        return true;
    }
    if !is_cell(recv) {
        return false;
    }
    unsafe {
        let p = as_void_ptr(recv);
        let t = (p.cast::<u8>().add(4) as *const u16).read();
        if t == Tag::Str as u16
            || t == Tag::Arr as u16
            || t == Tag::StringWrapper as u16
            || t == Tag::Map as u16
            || t == Tag::Set as u16
            || t == Tag::MapIter as u16
            || t == Tag::ArrIter as u16
            || t == Tag::IterHelper as u16
        {
            return true;
        }
        if t == Tag::Obj as u16 {
            let class_tag = p.cast::<u8>().add(8).cast::<u32>().read();
            let layout = __torajs_struct_layout_lookup(class_tag);
            return !layout.is_null()
                && (!__torajs_struct_method_find(
                    layout,
                    SYM_ITERATOR_METHOD.as_ptr(),
                    SYM_ITERATOR_METHOD.len() as u32,
                )
                .is_null()
                    || !__torajs_struct_method_find(
                        layout,
                        SYM_ASYNC_ITERATOR_METHOD.as_ptr(),
                        SYM_ASYNC_ITERATOR_METHOD.len() as u32,
                    )
                    .is_null());
        }
        false
    }
}
