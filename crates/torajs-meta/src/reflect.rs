//! Reflection helpers — `Object.getPrototypeOf(any)` +
//! `Object.getOwnPropertyDescriptor(obj, key)` — port of
//! `runtime_str.c` L494-549 + L881-915.
//!
//! Both helpers walk the AnyBox payload, branch on tag, and route
//! through `torajs-dynobj` for the slot reads. Returned values are
//! always owned Any-boxes (caller takes ownership).
//!
//! `get_property_descriptor` allocates a fresh dynobj with 4 fields
//! (`value` / `writable` / `enumerable` / `configurable`) before
//! wrapping it in an ANY_HEAP box. ANY_HEAP values in the source
//! dynobj are rc-incremented so the descriptor's `value` slot owns
//! its share independently.

use core::ffi::c_void;

unsafe extern "C" {
    fn __torajs_any_box(tag: i64, value: i64) -> *mut c_void;
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(child: *mut c_void);
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const u8) -> bool;
    fn __torajs_dynobj_get_tag(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_flags(dynobj: *const c_void, key: *const u8) -> u64;
}

// Tag values mirrored from torajs-anyvalue::AnySlotTag — re-declared
// here to keep this crate's dep tree narrow (no torajs-anyvalue
// Cargo dep; the i64 wire tag is part of the ABI anyway).
const ANY_NULL: i64 = 0;
const ANY_BOOL: i64 = 1;
const ANY_UNDEF: i64 = 5;
const ANY_HEAP: i64 = 4;

const ANY_BOX_TAG_OFF: usize = 8;
const ANY_BOX_VAL_OFF: usize = 16;

// Tag::DynObj from torajs-rc — universal heap header at offset 0.
const TAG_DYNOBJ: u16 = 14;

#[inline]
unsafe fn any_box_tag(box_ptr: *const c_void) -> i64 {
    unsafe { (box_ptr.cast::<u8>().add(ANY_BOX_TAG_OFF) as *const i64).read() }
}

#[inline]
unsafe fn any_box_value(box_ptr: *const c_void) -> i64 {
    unsafe { (box_ptr.cast::<u8>().add(ANY_BOX_VAL_OFF) as *const i64).read() }
}

#[inline]
unsafe fn heap_type_tag(child: *const c_void) -> u16 {
    // Universal heap header: refcount u32 at +0, type_tag u16 at +4.
    unsafe { child.cast::<u8>().add(4).cast::<u16>().read() }
}

#[inline]
unsafe fn alloc_str_key(name: &[u8]) -> *mut u8 {
    let s = unsafe { __torajs_str_alloc_pooled(name.len() as u64) };
    if !name.is_empty() {
        unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), s.add(16), name.len()) };
    }
    s
}

/// `Object.getPrototypeOf(any)` — reads `__proto__` from the box's
/// wrapped dynobj. Returns ANY_NULL box on tag mismatch / missing
/// __proto__. Identity-preserving: the returned ANY_HEAP box wraps
/// the SAME dynobj pointer the parent prototype was stored at, so
/// `getPrototypeOf(C.prototype) === B.prototype` holds via
/// any_payload_eq's ptr compare.
///
/// # Safety
/// `box_ptr` is NULL or a valid `*const AnyBox`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_get_proto_of_any(box_ptr: *const c_void) -> *mut c_void {
    if box_ptr.is_null() {
        return unsafe { __torajs_any_box(ANY_NULL, 0) };
    }
    let tag = unsafe { any_box_tag(box_ptr) };
    if tag != ANY_HEAP {
        return unsafe { __torajs_any_box(ANY_NULL, 0) };
    }
    let dynobj = unsafe { any_box_value(box_ptr) } as *mut c_void;
    if dynobj.is_null() {
        return unsafe { __torajs_any_box(ANY_NULL, 0) };
    }
    if unsafe { heap_type_tag(dynobj) } != TAG_DYNOBJ {
        return unsafe { __torajs_any_box(ANY_NULL, 0) };
    }
    let k = unsafe { alloc_str_key(b"__proto__") };
    if !unsafe { __torajs_dynobj_has(dynobj, k) } {
        unsafe { __torajs_str_drop(k) };
        return unsafe { __torajs_any_box(ANY_NULL, 0) };
    }
    let v_tag = unsafe { __torajs_dynobj_get_tag(dynobj, k) } as i64;
    let v_val = unsafe { __torajs_dynobj_get_value(dynobj, k) } as i64;
    unsafe { __torajs_str_drop(k) };
    unsafe { __torajs_any_box(v_tag, v_val) }
}

/// `Object.getOwnPropertyDescriptor(obj, key)` — builds a fresh
/// dynobj `{ value, writable, enumerable, configurable }` from the
/// source dynobj's slot, wraps it in an ANY_HEAP box.
///
/// ANY_HEAP-tagged slot values are rc-incremented so the descriptor
/// `value` field owns its share independently of the source. The
/// `writable` / `enumerable` / `configurable` booleans come from
/// the source dynobj's `flags` bitfield (`flags & 1` /  `>> 1` /
/// `>> 2`).
///
/// # Safety
/// `obj_any` and `key` are NULL or valid pointers per their type
/// (AnyBox + Str).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_get_property_descriptor(
    obj_any: *const c_void,
    key: *const c_void,
) -> *mut c_void {
    if obj_any.is_null() || key.is_null() {
        return unsafe { __torajs_any_box(ANY_UNDEF, 0) };
    }
    let tag = unsafe { any_box_tag(obj_any) };
    if tag != ANY_HEAP {
        return unsafe { __torajs_any_box(ANY_UNDEF, 0) };
    }
    let dynobj = unsafe { any_box_value(obj_any) } as *mut c_void;
    if dynobj.is_null() {
        return unsafe { __torajs_any_box(ANY_UNDEF, 0) };
    }
    if unsafe { heap_type_tag(dynobj) } != TAG_DYNOBJ {
        return unsafe { __torajs_any_box(ANY_UNDEF, 0) };
    }
    let k_str = key as *const u8;
    if !unsafe { __torajs_dynobj_has(dynobj, k_str) } {
        return unsafe { __torajs_any_box(ANY_UNDEF, 0) };
    }
    let v_tag = unsafe { __torajs_dynobj_get_tag(dynobj, k_str) };
    let v_val = unsafe { __torajs_dynobj_get_value(dynobj, k_str) };
    let flags = unsafe { __torajs_dynobj_get_flags(dynobj, k_str) };

    let mut desc = unsafe { __torajs_dynobj_alloc() };
    // ANY_HEAP value: bump the rc so the new descriptor field owns
    // its share independently of the source dynobj.
    if v_tag as i64 == ANY_HEAP {
        unsafe { __torajs_rc_inc(v_val as *mut c_void) };
    }

    let entries: [(&[u8], u64, u64); 4] = [
        (b"value", v_tag, v_val),
        (b"writable", ANY_BOOL as u64, (flags >> 0) & 1),
        (b"enumerable", ANY_BOOL as u64, (flags >> 1) & 1),
        (b"configurable", ANY_BOOL as u64, (flags >> 2) & 1),
    ];
    for &(name, t, v) in entries.iter() {
        let k = unsafe { alloc_str_key(name) };
        unsafe { __torajs_dynobj_set(&mut desc, k, t, v) };
        unsafe { __torajs_str_drop(k) };
    }
    let result = unsafe { __torajs_any_box(ANY_HEAP, desc as i64) };
    // any_box rc_inc'd desc → it's now 2 (our local + the box). Drop
    // our local so the box becomes the sole owner.
    unsafe { __torajs_value_drop_heap(desc) };
    result
}
