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

const ANY_BOX_VAL_OFF: usize = 16;

// HeapHeader: refcount u32 @+0, type_tag u16 @+4, flags u16 @+6.
// Step 5b+ packs the 4-bit AnySlotTag into `flags` bits 8..11; this
// crate mirrors the bit positions from torajs-rc rather than taking
// a Cargo dep (deps tree stays narrow — see Cargo.toml comment).
const ANY_BOX_FLAGS_OFF: usize = 6;
const ANY_TAG_SHIFT: u16 = 8;
const ANY_TAG_MASK: u16 = 0b1111 << ANY_TAG_SHIFT;

// Tag::DynObj from torajs-rc — universal heap header at offset 0.
const TAG_DYNOBJ: u16 = 14;

#[inline]
unsafe fn any_box_tag(box_ptr: *const c_void) -> i64 {
    let flags = unsafe { (box_ptr.cast::<u8>().add(ANY_BOX_FLAGS_OFF) as *const u16).read() };
    ((flags & ANY_TAG_MASK) >> ANY_TAG_SHIFT) as i64
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

// ============================================================
// Step 7d — `__torajs_anyv_*` variants. Same logic as the old
// `*AnyBox`-shape fns but operate on NaN-box `AnyValue`
// immediates instead. ssa_lower migrates to these after 7d-A
// atomic switch; old shims stay for link compat.
// ============================================================

// NaN-box constants mirrored from torajs-anyvalue::nanbox.
const VALUE_NULL_IMM: u64 = 0x02;
const VALUE_UNDEFINED_IMM: u64 = 0x0A;
const VALUE_FALSE_IMM: u64 = 0x06;
const VALUE_TRUE_IMM: u64 = 0x07;
const TAG_TYPE_NUMBER: u64 = 0xFFFE_0000_0000_0000;
const TAG_BIT_TYPE_OTHER: u64 = 0x02;
const DOUBLE_ENCODE_OFFSET: u64 = 0x0007_0000_0000_0000;

#[inline]
const fn is_cell_imm(v: u64) -> bool {
    (v & TAG_TYPE_NUMBER) == 0 && (v & TAG_BIT_TYPE_OTHER) == 0 && v != 0
}

#[inline]
fn box_pair_imm(tag: i64, value: i64) -> u64 {
    match tag {
        0 => VALUE_NULL_IMM,
        1 => {
            if value != 0 {
                VALUE_TRUE_IMM
            } else {
                VALUE_FALSE_IMM
            }
        }
        2 => {
            if let Ok(n32) = i32::try_from(value) {
                TAG_TYPE_NUMBER | (n32 as u32 as u64)
            } else {
                (value as f64).to_bits().wrapping_add(DOUBLE_ENCODE_OFFSET)
            }
        }
        3 => (value as u64).wrapping_add(DOUBLE_ENCODE_OFFSET),
        4 => {
            if value == 0 {
                VALUE_NULL_IMM
            } else {
                value as u64
            }
        }
        5 => VALUE_UNDEFINED_IMM,
        _ => VALUE_NULL_IMM,
    }
}

/// AnyValue-immediate `Object.getPrototypeOf(any)` — same as
/// [`__torajs_get_proto_of_any`] but operates on a NaN-box
/// immediate input and returns a NaN-box immediate.
///
/// # Safety
///
/// `v` carries a valid AnyValue bit pattern; cell case must
/// point to a valid heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_get_proto_of_any(v: u64) -> u64 {
    if !is_cell_imm(v) {
        return VALUE_NULL_IMM;
    }
    let dynobj = v as *const c_void;
    // SAFETY: cell pointer to valid heap object per invariant.
    if unsafe { heap_type_tag(dynobj) } != TAG_DYNOBJ {
        return VALUE_NULL_IMM;
    }
    let k = unsafe { alloc_str_key(b"__proto__") };
    if !unsafe { __torajs_dynobj_has(dynobj, k) } {
        unsafe { __torajs_str_drop(k) };
        return VALUE_NULL_IMM;
    }
    let v_tag = unsafe { __torajs_dynobj_get_tag(dynobj, k) } as i64;
    let v_val = unsafe { __torajs_dynobj_get_value(dynobj, k) } as i64;
    unsafe { __torajs_str_drop(k) };
    // rc_inc heap payload — caller owns the returned reference.
    if v_tag == ANY_HEAP && v_val != 0 {
        // SAFETY: ANY_HEAP slot holds a valid heap pointer.
        unsafe { __torajs_rc_inc(v_val as *mut c_void) };
    }
    box_pair_imm(v_tag, v_val)
}

/// AnyValue-immediate `Object.getOwnPropertyDescriptor(obj, key)`
/// — same as [`__torajs_get_property_descriptor`] but operates
/// on a NaN-box immediate input and returns a NaN-box immediate.
///
/// # Safety
///
/// `obj_any` carries a valid AnyValue bit pattern; `key` is NULL
/// or a valid Str pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_get_property_descriptor(
    obj_any: u64,
    key: *const c_void,
) -> u64 {
    if !is_cell_imm(obj_any) || key.is_null() {
        return VALUE_UNDEFINED_IMM;
    }
    let dynobj = obj_any as *const c_void;
    // SAFETY: cell pointer to valid heap object.
    if unsafe { heap_type_tag(dynobj) } != TAG_DYNOBJ {
        return VALUE_UNDEFINED_IMM;
    }
    let k_str = key as *const u8;
    if !unsafe { __torajs_dynobj_has(dynobj, k_str) } {
        return VALUE_UNDEFINED_IMM;
    }
    let v_tag = unsafe { __torajs_dynobj_get_tag(dynobj, k_str) };
    let v_val = unsafe { __torajs_dynobj_get_value(dynobj, k_str) };
    let flags = unsafe { __torajs_dynobj_get_flags(dynobj, k_str) };

    let mut desc = unsafe { __torajs_dynobj_alloc() };
    if v_tag as i64 == ANY_HEAP && v_val != 0 {
        // SAFETY: ANY_HEAP slot holds a valid heap pointer.
        unsafe { __torajs_rc_inc(v_val as *mut c_void) };
    }

    let entries: [(&[u8], u64, u64); 4] = [
        (b"value", v_tag, v_val),
        (b"writable", ANY_BOOL as u64, flags & 1),
        (b"enumerable", ANY_BOOL as u64, (flags >> 1) & 1),
        (b"configurable", ANY_BOOL as u64, (flags >> 2) & 1),
    ];
    for &(name, t, val) in entries.iter() {
        let k = unsafe { alloc_str_key(name) };
        unsafe { __torajs_dynobj_set(&mut desc, k, t, val) };
        unsafe { __torajs_str_drop(k) };
    }
    // desc owns rc=1 from dynobj_alloc; transferred to caller
    // via the returned cell-encoded AnyValue (pre-7d the AnyBox-
    // wrapped path rc_inc'd + dropped the local; both cancel
    // out and we skip both).
    desc as u64
}
