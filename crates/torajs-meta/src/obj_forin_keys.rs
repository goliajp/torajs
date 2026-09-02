//! `__torajs_anyv_forin_keys` — the for-in keys source (chunk B2,
//! RFC 20260711 + the §14.7.5.9 user-[[Prototype]] chain walk of RFC
//! 20260721-object-descriptor-cluster 刀 5 R-F). Split from
//! `obj_own_keys.rs` (file-size limit) — the shared own-keys walk,
//! tag mirrors, and key filters stay there.

use core::ffi::c_void;

use crate::obj_own_keys::{
    __torajs_anyv_own_keys, ARR_DATA_PTR_OFF, ARR_LEN_OFF, FLAG_ENUMERABLE, TAG_DYNOBJ,
    TAG_OBJ_CELL, heap_type_tag_local, is_dynobj_imm, key_is_proto_slot,
};

unsafe extern "C" {
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-dynobj iteration surface — keys are BORROWED.
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    /// torajs-anyvalue — borrowed user-[[Prototype]] cell of a DynObj
    /// (NULL = null-proto / implicit chain / non-cell).
    fn __torajs_dynobj_user_proto(dynobj: *const c_void) -> *mut c_void;
}

/// Borrowed user-[[Prototype]] cell of a DynObj — thin wrapper over
/// the torajs-anyvalue [[Prototype]]-slot single source
/// (`__torajs_dynobj_user_proto`). `None` for the null-proto shape,
/// an absent entry (the implicit %Object.prototype% chain), or a
/// non-cell payload.
///
/// # Safety
/// `obj` is a live `TAG_DYNOBJ` heap pointer.
unsafe fn user_proto_cell_borrowed(obj: *const c_void) -> Option<*const c_void> {
    let parent = unsafe { __torajs_dynobj_user_proto(obj) };
    if parent.is_null() {
        None
    } else {
        Some(parent as *const c_void)
    }
}

/// Content equality of two live key cells, by WTF-8 spelling — a
/// Symbol key (never yielded by for-in, but recorded as a shadow)
/// matches only itself.
unsafe fn key_bytes_eq(a: *const c_void, b: *const c_void) -> bool {
    if a == b {
        return true;
    }
    if unsafe { heap_type_tag_local(a) == crate::reflect::TAG_SYMBOL }
        || unsafe { heap_type_tag_local(b) == crate::reflect::TAG_SYMBOL }
    {
        return false;
    }
    unsafe { crate::str_wtf8::StrWtf8::of(a) }.as_bytes()
        == unsafe { crate::str_wtf8::StrWtf8::of(b) }.as_bytes()
}

/// One DynObj level of the §14.7.5.9 walk: yield enumerable keys not
/// yet shadowed (`yield_keys` — false for level 0, whose enumerable
/// face `anyv_own_keys` already emitted), record EVERY own key into
/// the visited set (a non-enumerable own key still shadows deeper
/// levels). Returns the (possibly reallocated) out array.
unsafe fn forin_visit_dynobj(
    level: *const c_void,
    yield_keys: bool,
    visited: &mut Vec<*mut c_void>,
    mut out: *mut u8,
) -> *mut u8 {
    let len = unsafe { __torajs_dynobj_iter_len(level) };
    let mut order = vec![0u64; len as usize];
    let n = unsafe { __torajs_dynobj_iter_order(level, order.as_mut_ptr(), len) };
    for &i in order.iter().take(n as usize) {
        let key = unsafe { __torajs_dynobj_iter_key(level, i) };
        if key.is_null() || unsafe { key_is_proto_slot(key) } {
            continue;
        }
        if yield_keys
            && unsafe { __torajs_dynobj_iter_flags(level, i) } & FLAG_ENUMERABLE != 0
            && !visited
                .iter()
                .any(|&k| unsafe { key_bytes_eq(k as *const c_void, key as *const c_void) })
        {
            // Borrowed key → the array slot takes its own share.
            unsafe { __torajs_rc_inc(key) };
            out = unsafe { __torajs_arr_push(out, key as i64) };
        }
        visited.push(key);
    }
    out
}

/// Append every key of an owned `Arr<Str>` not in the visited set
/// onto `out` (each pushed slot takes its own share; the source
/// array keeps its own references for the caller to drop).
unsafe fn append_unvisited(
    keys: *const c_void,
    visited: &[*mut c_void],
    mut out: *mut u8,
) -> *mut u8 {
    let len = unsafe { (keys.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    let data = unsafe {
        keys.cast::<u8>()
            .add(ARR_DATA_PTR_OFF)
            .cast::<*mut u64>()
            .read()
    };
    if data.is_null() {
        return out;
    }
    for i in 0..len {
        let s = unsafe { data.add(i as usize).read() } as *mut c_void;
        if s.is_null() {
            continue;
        }
        if visited
            .iter()
            .any(|&k| unsafe { key_bytes_eq(k as *const c_void, s as *const c_void) })
        {
            continue;
        }
        unsafe { __torajs_rc_inc(s) };
        out = unsafe { __torajs_arr_push(out, s as i64) };
    }
    out
}

/// for-in keys source (chunk B2, RFC 20260711 + 20260721 刀 5 R-F):
/// level 0 is the `Object.keys` enumerable-own surface; a DynObj
/// receiver then walks its user [[Prototype]] chain per §14.7.5.9
/// EnumerateObjectProperties — each deeper level's enumerable keys
/// append unless shadowed by ANY own key (enumerable or not) of a
/// nearer level. A null / undefined receiver enumerates nothing —
/// ES §14.7.5 ForIn/OfHeadEvaluation step 3 short-circuits before
/// ToObject can throw.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern; the caller owns the
/// returned `+1`-rc `Arr<Str>`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_forin_keys(v: u64) -> *mut c_void {
    if v == crate::reflect::VALUE_NULL_IMM || v == crate::reflect::VALUE_UNDEFINED_IMM {
        return unsafe { __torajs_arr_alloc(0) as *mut c_void };
    }
    let out = unsafe { __torajs_anyv_own_keys(v, 0) };
    if !is_dynobj_imm(v) {
        return out;
    }
    let mut out = out as *mut u8;
    // The visited set borrows key cells — every level stays live
    // through the receiver's chain references for the whole call.
    let mut visited: Vec<*mut c_void> = Vec::new();
    out = unsafe { forin_visit_dynobj(v as *const c_void, false, &mut visited, out) };
    let mut level = unsafe { user_proto_cell_borrowed(v as *const c_void) };
    let mut depth = 0usize;
    while let Some(cell) = level {
        // A simulated-slot cycle must not hang the kernel — cap
        // mirrors escape.rs's ELEM_DEPTH_CAP posture.
        depth += 1;
        if depth > 64 {
            break;
        }
        match unsafe { heap_type_tag_local(cell) } {
            TAG_DYNOBJ => {
                out = unsafe { forin_visit_dynobj(cell, true, &mut visited, out) };
                level = unsafe { user_proto_cell_borrowed(cell) };
            }
            TAG_OBJ_CELL => {
                // Struct level (`Object.create(structLit)`) — its
                // enumerable field keys yield, then the chain ends
                // (struct cells carry no user-proto simulation slot).
                let keys = unsafe { crate::struct_enum::__torajs_anyv_struct_keys(cell as u64, 0) };
                out = unsafe { append_unvisited(keys, &visited, out) };
                unsafe { __torajs_value_drop_heap(keys) };
                break;
            }
            _ => break,
        }
    }
    out as *mut c_void
}
