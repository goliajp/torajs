//! The expando-dict `length` probes `__torajs_any_length_get` runs
//! before answering a shape's inherent length or falling to
//! undefined: an own entry on a wrapper / buffer cell, and the family
//! prototype's entry for a wrapper, a primitive immediate or a
//! Symbol / BigInt cell (`Number.prototype.length = 2` then
//! `(1).length` — §10.1.8.1 walks the chain for `length` like any
//! other name; test262 `Array/fromAsync/asyncitems-number`).

use core::ffi::c_void;

use crate::nanbox::AnyValue;

unsafe extern "C" {
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
}

/// `props["length"]` when the dict holds the entry (a stored
/// undefined counts — it shadows the shape), boxed with the getter
/// run over `recv`. `None` for a null dict or an absent entry.
///
/// # Safety
/// `props` is null or a live dynobj; `recv` a live any value.
pub(crate) unsafe fn props_length_probe(props: *const c_void, recv: AnyValue) -> Option<AnyValue> {
    if props.is_null() {
        return None;
    }
    unsafe {
        let key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6) as *mut c_void;
        let dtag = __torajs_dynobj_get_tag(props, key);
        let dval = __torajs_dynobj_get_value(props, key);
        let present = dtag != 5 || __torajs_dynobj_has(props, key) != 0;
        let v = present.then(|| crate::len_get::box_probe_pair(dtag, dval, recv, key));
        __torajs_str_drop(key);
        v
    }
}

/// The family prototype's expando `length` (`recv_proto_family`
/// row space); `None` for a family-less receiver.
///
/// # Safety
/// `recv` is a live any value.
pub(crate) unsafe fn family_proto_length_probe(family: i64, recv: AnyValue) -> Option<AnyValue> {
    if family < 0 {
        return None;
    }
    let proto = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(family) }
        as *const c_void;
    unsafe { props_length_probe(proto, recv) }
}
