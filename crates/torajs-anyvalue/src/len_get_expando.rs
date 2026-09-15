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

/// The `Tag::DynObj` arm of `__torajs_any_length_get` — moved here
/// when that file reached its size limit (rotation 655).
///
/// # Safety
/// `ptr` is a live `Tag::DynObj` cell and `recv` its box.
pub(crate) unsafe fn dynobj_length(ptr: *mut c_void, recv: AnyValue) -> AnyValue {
    unsafe {
        // Own-property probe for the literal key "length" — a
        // user `{ length: 5 }` through any answers its value;
        // absence answers (5, 0) = undefined by construction; an
        // accessor entry runs its getter (box_probe_pair).
        let key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6);
        let dtag = __torajs_dynobj_get_tag(ptr, key as *const c_void);
        let dval = __torajs_dynobj_get_value(ptr, key as *const c_void);
        // 刀 6 G9d — a truly absent own entry continues along the
        // user [[Prototype]] chain (§10.1.8.1 OrdinaryGet):
        // `Object.create(arr).length` answers the Arr parent's
        // inherent length. The recursion covers grandparents
        // (member_get knife-2 shape — a heap cell's pointer bits
        // ARE its box encoding).
        if dtag == 5 && __torajs_dynobj_has(ptr, key as *const c_void) == 0 {
            if let Some(parent) = crate::member_get_own::user_proto_cell(ptr) {
                __torajs_str_drop(key as *mut c_void);
                return crate::len_get::__torajs_any_length_get(parent);
            }
            // Function.prototype's virtual own `length` 0
            // (§20.2.3, RFC 20260722 刀 3).
            if let Some((mtag, mval)) =
                crate::method_support_proto_meta::builtin_proto_own_meta(ptr, key as *const c_void)
            {
                __torajs_str_drop(key as *mut c_void);
                return crate::nanbox_encode::__torajs_anyv_box_from_pair(mtag as i64, mval as i64);
            }
        }
        let v = crate::len_get::box_probe_pair(dtag, dval, recv, key as *const c_void);
        __torajs_str_drop(key as *mut c_void);
        v
    }
}
