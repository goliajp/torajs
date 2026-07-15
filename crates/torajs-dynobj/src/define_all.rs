//! `Object.defineProperties(obj, props)` / `Object.create(proto,
//! props)` — runtime props walk (RFC 20260712-object-create-define-props
//! chunk 2).
//!
//! Spec §20.1.2.3.1 ObjectDefineProperties: walk the props object's
//! enumerable own keys in OwnPropertyKeys order, ToPropertyDescriptor
//! each value (must be an object, else TypeError), then apply every
//! descriptor. The two phases are kept separate exactly as the spec
//! writes them — all descriptors are read **before** the first define
//! fires, which also makes `defineProperties(o, o)` safe (defines
//! resize `obj`'s dense array; a single-phase walk over `props ==
//! obj` would iterate a freed buffer).
//!
//! Each per-key define routes through
//! [`crate::define::__torajs_dynobj_define_from_desc`] (data +
//! accessor descriptors, pending-TypeError on validation failure);
//! the walk stops at the first pending throw.

use core::ffi::c_void;

use crate::define_from_desc::__torajs_dynobj_define_from_desc;
use crate::get::type_tag;
use crate::layout::{
    BUCKET_FLAG_ENUMERABLE, DYNOBJ_KEY_HOLE, TAG_ARR_HDR, TAG_CLOSURE_HDR, TAG_DYNOBJ, TAG_OBJ,
};
use crate::probe::{bucket_flags, bucket_key_ptr, entries, entries_len};

unsafe extern "C" {
    fn calloc(size: usize) -> *mut c_void;
    fn free(p: *mut c_void, size: usize);
    fn __torajs_throw_type_error(msg: *const u8);
    fn __torajs_throw_check() -> i64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
}

/// NaN-box slot tag mirror (`torajs_anyvalue::AnySlotTag::Heap`).
const ANY_HEAP: u64 = 4;

/// `__torajs_dynobj_define_properties_from(obj_slot, props)` — apply
/// every enumerable own entry of the `props` dynobj as a property
/// descriptor on `*obj_slot`. Keys are borrowed from `props` entries
/// (`define_apply` incs on a fresh define); descriptor values must be
/// dynobj cells (else the §20.1.2.3.1 step-5b TypeError).
///
/// # Safety
/// `obj_slot` points at a live `*mut c_void` (dynobj or NULL).
/// `props` is a dynobj heap pointer or NULL. Caller must check for
/// pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_define_properties_from(
    obj_slot: *mut *mut c_void,
    props: *const c_void,
) {
    if props.is_null() {
        return;
    }
    // Only dynobj-backed receivers carry define storage (Arr /
    // typed receivers are the RFC backlog) — a foreign cell in the
    // slot must not be probed as a dynobj dense array.
    let obj = unsafe { *obj_slot };
    if obj.is_null() || unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return;
    }
    if unsafe { type_tag(props) } != TAG_DYNOBJ {
        unsafe {
            __torajs_throw_type_error(
                c"Property description must be an object.".as_ptr() as *const u8
            );
        }
        return;
    }
    let len = unsafe { entries_len(props) } as usize;
    if len == 0 {
        return;
    }
    // Phase 1 — collect (key, desc) pairs for every enumerable own
    // entry, validating each descriptor is an object.
    let buf_bytes = len * 16;
    let buf = unsafe { calloc(buf_bytes) } as *mut u64;
    let mut n = 0usize;
    for i in 0..len {
        let e = unsafe { entries(props).add(i) };
        let kp_tagged = unsafe { (*e).key_ptr_tagged };
        if kp_tagged == DYNOBJ_KEY_HOLE {
            continue;
        }
        if bucket_flags(kp_tagged) & BUCKET_FLAG_ENUMERABLE == 0 {
            continue;
        }
        let desc_anyv = unsafe { (*e).value_anyv };
        let d_tag = unsafe { __torajs_anyv_unbox_tag(desc_anyv) } as u64;
        let d_val = unsafe { __torajs_anyv_unbox_value(desc_anyv) } as u64;
        // 刀 3 (RFC 20260714-t262-top-clusters) — §8.10.5
        // ToPropertyDescriptor accepts ANY object: a Closure / Arr
        // descriptor (test262's `descObj = function(){};
        // descObj.configurable = true`) reads its expando fields —
        // `define_from_desc` already dispatches per desc-cell shape,
        // so the gate here matches its accept set instead of
        // dynobj-only.
        //
        // RFC 20260716 刀 22 — extend accept to include `TAG_OBJ`
        // (static-layout ObjectLit cells) so a desc value like
        // `{value: {}, enumerable: true}` propagated through
        // `Object.defineProperty(props, ...)` clears this gate; the
        // dispatcher's `_ => null` fallback treats such a cell as an
        // empty descriptor (create with all-false flags,
        // `value = undefined`). Str / Symbol / AccessorPair /
        // BigInt / Wrapper cells stay OUT of the accept set — a
        // primitive string `"abc"` descriptor and a "no-getter" own
        // accessor entry (spec-Get(props, key) = undefined) must
        // both hit the §6.2.6.5 step-1 TypeError, not the empty-desc
        // fallback (regression witnessed by
        // `test262:S15.2.3.5-4-{26,45}` when the accept was widened
        // to all heap cells).
        let desc_ok = d_tag == ANY_HEAP
            && d_val != 0
            && matches!(
                unsafe { type_tag(d_val as *const c_void) },
                TAG_DYNOBJ | TAG_CLOSURE_HDR | TAG_ARR_HDR | TAG_OBJ
            );
        if !desc_ok {
            unsafe {
                __torajs_throw_type_error(
                    c"Property description must be an object.".as_ptr() as *const u8
                );
                free(buf as *mut c_void, buf_bytes);
            }
            return;
        }
        unsafe {
            *buf.add(n * 2) = bucket_key_ptr(kp_tagged) as u64;
            *buf.add(n * 2 + 1) = d_val;
        }
        n += 1;
    }
    // Phase 2 — apply in collection order; stop at the first pending
    // throw (non-configurable redefine / bad accessor field).
    for j in 0..n {
        let key = unsafe { *buf.add(j * 2) } as *mut c_void;
        let desc = unsafe { *buf.add(j * 2 + 1) } as *const c_void;
        unsafe { __torajs_dynobj_define_from_desc(obj_slot, key, desc) };
        if unsafe { __torajs_throw_check() } != 0 {
            break;
        }
    }
    unsafe { free(buf as *mut c_void, buf_bytes) };
}
