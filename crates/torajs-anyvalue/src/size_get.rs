//! `recv.size` on an `any` receiver — the Map / Set live entry count
//! and the own-property probe every other shape falls back to.
//!
//! Split from [`crate::len_get`] at the file cap: that module answers
//! "how long is this", this one answers "how many entries does it
//! have", and the two share nothing but the shape of the dispatch.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null, is_undefined};

unsafe extern "C" {
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(p: *mut c_void);
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_map_size(p: *const c_void) -> i64;
}

/// `recv.size` where the receiver is an `any` value
/// (Any-method-call RFC 20260704 C4-2).
///
/// - `null` / `undefined` → catchable TypeError.
/// - `Tag::Map` / `Tag::Set` → live entry count (the ES §24.1.3.10 /
///   §24.2.3.9 `size` accessors) via `__torajs_map_size`.
/// - everything else → an ordinary [[Get]] of `"size"`.
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_size_get(recv: AnyValue) -> AnyValue {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    let ptr = as_void_ptr(recv);
    // SAFETY: a cell box points at a live heap header (tag at +4).
    let tag = if is_cell(recv) {
        unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() }
    } else {
        u16::MAX
    };
    if tag == Tag::Map as u16 || tag == Tag::Set as u16 {
        unsafe {
            // §10.1.8.1 own-before-family: a `defineProperty(m, "size",
            // …)` lands in the cell's own bag and shadows the family
            // accessor. The keyed spelling reached the bag through the
            // general cascade and answered it; this intrinsic went
            // straight to the live count, so `m["size"]` and `m.size`
            // disagreed about the same object.
            let props = crate::member_get_layout::expando_props(ptr, tag);
            if !props.is_null() {
                let key = __torajs_str_alloc(c"size".as_ptr() as *const u8, 4);
                let present = __torajs_dynobj_has(props, key as *const c_void) != 0;
                let dtag = __torajs_dynobj_get_tag(props, key as *const c_void);
                let dval = __torajs_dynobj_get_value(props, key as *const c_void);
                if present {
                    let v = crate::len_get::box_probe_pair(dtag, dval, recv, key as *const c_void);
                    __torajs_str_drop(key as *mut c_void);
                    return v;
                }
                __torajs_str_drop(key as *mut c_void);
            }
            return crate::nanbox_encode::__torajs_anyv_box_i64(__torajs_map_size(ptr));
        }
    }
    // Everything else is an ordinary [[Get]] of "size": a struct field
    // or accessor, a class or exotic-subclass method, a dynobj entry,
    // what the receiver inherits (`Object.create(m)` reads the Map's
    // size through its chain; a patched `Array.prototype.size`).
    unsafe {
        let key = __torajs_str_alloc(c"size".as_ptr() as *const u8, 4);
        let v = crate::member_get_receiver::__torajs_any_member_get_with_receiver(
            recv,
            key as *const c_void,
            recv,
        );
        __torajs_str_drop(key as *mut c_void);
        v
    }
}
