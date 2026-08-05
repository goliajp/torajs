//! Struct-cell own-keys merge — RFC 20260714-struct-dynamic-props
//! blade 3 (enumeration face over blade 2's +24 expando dict).
//!
//! The typed keys / for-in / gOPN emits mint the LAYOUT field-name
//! list at compile time and hand it to `__torajs_obj_own_keys`; a
//! struct cell that grew expandos must fold those in per ES
//! §10.1.11.1 OrdinaryOwnPropertyKeys:
//!
//! 1. every integer-like key — layout field names spelled as
//!    canonical indices (`{0:"a"}` anon structs) AND numeric
//!    expandos — in ascending numeric order,
//! 2. the remaining layout names in declaration order (all predate
//!    every expando: fields exist from construction),
//! 3. the remaining expando keys in insertion order.
//!
//! A NULL props slot short-circuits to the static list untouched, so
//! the no-expando path stays allocation-free.

use core::ffi::c_void;

use crate::obj_own_keys::FLAG_ENUMERABLE;
use crate::obj_own_keys_key_shape::{key_is_canonical_index, key_is_proto_slot};

unsafe extern "C" {
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    // fnprops' spelling — the key param is the live Str CELL pointer.
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const u8) -> u64;
}

/// `torajs-core ssa_lower::OBJ_PROPS_OFF` mirror — the struct cell's
/// lazily-allocated expando dict (blade 1).
pub(crate) const OBJ_PROPS_OFF: usize = 24;

/// The struct cell's props dict, NULL when no expando was written.
pub(crate) unsafe fn struct_props(cell: *const c_void) -> *const c_void {
    unsafe { *(cell.cast::<u8>().add(OBJ_PROPS_OFF) as *const u64) as *const c_void }
}

/// Walk the struct cell's ENUMERABLE expando entries in insertion
/// order, handing each `(key Str cell, value tag, value payload)` to
/// `f`. Every handed pointer/pair is a BORROW — the callback takes
/// its own shares. NULL / empty props is a no-op. Shared by the
/// values / entries walks (torajs-meta `struct_enum`).
pub(crate) unsafe fn for_each_enumerable_expando(
    cell: *const c_void,
    mut f: impl FnMut(*mut c_void, u64, u64),
) {
    let props = unsafe { struct_props(cell) };
    if props.is_null() {
        return;
    }
    let n_props = unsafe { __torajs_dynobj_iter_len(props) };
    if n_props == 0 {
        return;
    }
    let mut order = vec![0u64; n_props as usize];
    let n = unsafe { __torajs_dynobj_iter_order(props, order.as_mut_ptr(), n_props) };
    for &i in order.iter().take(n as usize) {
        if unsafe { __torajs_dynobj_iter_flags(props, i) } & FLAG_ENUMERABLE == 0 {
            continue;
        }
        let key = unsafe { __torajs_dynobj_iter_key(props, i) };
        if key.is_null() || unsafe { key_is_proto_slot(key) } {
            continue;
        }
        // RFC 20260806-declared-field-redefine — a declared name in
        // the dict is the field's ATTRIBUTE SIDECAR, not a second own
        // property, and its `value_anyv` is filler. The field itself
        // is enumerated from the layout.
        if unsafe { crate::struct_field_attrs::layout_declares(cell, key) } {
            continue;
        }
        let tag = unsafe { __torajs_dynobj_get_tag(props, key.cast::<u8>()) };
        let value = unsafe { __torajs_dynobj_get_value(props, key.cast::<u8>()) };
        f(key, tag, value);
    }
}

/// Numeric value of a canonical-index key (caller checked the shape).
unsafe fn index_key_value(key: *const c_void) -> u64 {
    let len = unsafe { key.cast::<u8>().add(8).cast::<u32>().read() } as usize;
    let bytes = unsafe { core::slice::from_raw_parts(key.cast::<u8>().add(16), len) };
    let mut v: u64 = 0;
    for &b in bytes {
        v = v * 10 + (b - b'0') as u64;
    }
    v
}

/// Fold a struct cell's expando keys into its static layout-name
/// list per the module-doc order. Consumes `static_names` (an owned
/// fresh `Arr<Str>`); answers an owned array either way.
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell; `static_names` is an owned
/// heap-pointer-kind `Arr<Str>` this call may consume.
pub(crate) unsafe fn struct_keys_with_expandos(
    cell: *const c_void,
    static_names: *mut c_void,
    include_nonenum: i64,
) -> *mut c_void {
    let props = unsafe { struct_props(cell) };
    if props.is_null() {
        return static_names;
    }
    let n_props = unsafe { __torajs_dynobj_iter_len(props) };
    if n_props == 0 {
        return static_names;
    }
    // Collect the expando keys in ES dict order (BORROWED cells).
    let mut order = vec![0u64; n_props as usize];
    let n = unsafe { __torajs_dynobj_iter_order(props, order.as_mut_ptr(), n_props) };
    let mut exp_ints: Vec<(u64, *mut c_void)> = Vec::new();
    let mut exp_tail: Vec<*mut c_void> = Vec::new();
    for &i in order.iter().take(n as usize) {
        if include_nonenum == 0 {
            let flags = unsafe { __torajs_dynobj_iter_flags(props, i) };
            if flags & FLAG_ENUMERABLE == 0 {
                continue;
            }
        }
        let key = unsafe { __torajs_dynobj_iter_key(props, i) };
        if key.is_null() || unsafe { key_is_proto_slot(key) } {
            continue;
        }
        // A declared name in the dict is the field's attribute
        // sidecar (RFC 20260806-declared-field-redefine) — the field
        // is already in `static_names`, and listing the sidecar
        // beside it would be the same key twice.
        if unsafe { crate::struct_field_attrs::layout_declares(cell, key) } {
            continue;
        }
        if unsafe { key_is_canonical_index(key) } {
            exp_ints.push((unsafe { index_key_value(key) }, key));
        } else {
            exp_tail.push(key);
        }
    }
    // Split the static list the same way (BORROWED cells into the
    // owned source array — the rebuilt array takes fresh shares and
    // the source drops whole at the end).
    let s_len = unsafe {
        static_names
            .cast::<u8>()
            .add(crate::obj_own_keys::ARR_LEN_OFF)
            .cast::<u64>()
            .read()
    };
    let s_data = unsafe {
        static_names
            .cast::<u8>()
            .add(crate::obj_own_keys::ARR_DATA_PTR_OFF)
            .cast::<*const u64>()
            .read()
    };
    let mut int_keys: Vec<(u64, *mut c_void)> = Vec::new();
    let mut static_tail: Vec<*mut c_void> = Vec::new();
    let mut dropped_static = false;
    for i in 0..s_len {
        let key = unsafe { s_data.add(i as usize).read() } as *mut c_void;
        if key.is_null() {
            continue;
        }
        // A field redefined to `enumerable: false` leaves Object.keys
        // and for-in, but `getOwnPropertyNames` still wants it — the
        // sidecar is the only place that distinction is recorded, the
        // layout row having no room for it.
        if include_nonenum == 0
            && unsafe { crate::struct_field_attrs::sidecar_attrs(cell, key) }
                .is_some_and(|attrs| attrs & FLAG_ENUMERABLE == 0)
        {
            dropped_static = true;
            continue;
        }
        if unsafe { key_is_canonical_index(key) } {
            int_keys.push((unsafe { index_key_value(key) }, key));
        } else {
            static_tail.push(key);
        }
    }
    // Nothing folded in and nothing filtered out — hand the static
    // list back untouched rather than rebuild it verbatim.
    if exp_ints.is_empty() && exp_tail.is_empty() && !dropped_static {
        return static_names;
    }
    int_keys.extend(exp_ints);
    int_keys.sort_by_key(|(v, _)| *v);
    let total = int_keys.len() + static_tail.len() + exp_tail.len();
    let mut out = unsafe { __torajs_arr_alloc(total as u64) };
    for key in int_keys
        .iter()
        .map(|(_, k)| *k)
        .chain(static_tail)
        .chain(exp_tail)
    {
        unsafe { __torajs_rc_inc(key) };
        out = unsafe { __torajs_arr_push(out, key as i64) };
    }
    unsafe { __torajs_value_drop_heap(static_names) };
    out as *mut c_void
}
