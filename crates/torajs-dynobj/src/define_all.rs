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

use crate::accessor::TAG_ACCESSOR_PAIR;
use crate::define_from_desc::__torajs_dynobj_define_from_desc;
use crate::get::type_tag;
use crate::layout::{
    BUCKET_FLAG_ENUMERABLE, CELL_PROPS_OFF, DYNOBJ_KEY_HOLE, TAG_ARR_HDR, TAG_CLOSURE_HDR,
    TAG_DYNOBJ, TAG_OBJ,
};

/// Primitive-wrapper tags (RFC 20260716 刀 2b / 刀 5). All three share
/// the `[header:8][value:8][props:8]` layout; the expando dynobj slot
/// lives at `+16` (mirror of `torajs-wrapper::WRAPPER_PROPS_OFF`).
const TAG_NUMBER_WRAPPER: u16 = 21;
const TAG_STRING_WRAPPER: u16 = 22;
const TAG_BOOLEAN_WRAPPER: u16 = 23;
const WRAPPER_PROPS_OFF: usize = 16;
use crate::probe::{bucket_flags, bucket_key_ptr, entries, entries_len};

unsafe extern "C" {
    fn calloc(size: usize) -> *mut c_void;
    fn free(p: *mut c_void, size: usize);
    fn __torajs_throw_type_error(msg: *const u8);
    fn __torajs_throw_check() -> i64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_accessor_invoke_getter(pair: *const c_void) -> u64;
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-meta struct_enum — own enumerable layout keys of a
    /// `TAG_OBJ` struct cell as a fresh `Arr<Str>` (minted keys).
    fn __torajs_anyv_struct_keys(v: u64) -> *mut c_void;
    /// torajs-anyvalue member probes — the struct arm answers layout
    /// fields borrow-shaped, accessors via the tag-6 sentinel.
    fn __torajs_any_member_get_tag(recv: u64, key: *const c_void) -> u64;
    fn __torajs_any_member_get_value(recv: u64, key: *const c_void) -> u64;
    fn __torajs_any_accessor_get(recv: u64, key: *const c_void, pair_bits: u64) -> u64;
}

/// `struct_probe::ANY_ACCESSOR_TAG` mirror — member-get tag channel's
/// accessor sentinel.
const ANY_ACCESSOR_TAG: u64 = 6;

/// torajs-arr cell layout mirrors (`layout.rs` B1 fixed cell).
const ARR_LEN_OFF: usize = 8;
const ARR_DATA_PTR_OFF: usize = 32;

/// NaN-box slot tag mirror (`torajs_anyvalue::AnySlotTag::Heap`).
const ANY_HEAP: u64 = 4;

/// Per-entry stride in the Phase-1 buffer: `[key_ptr, desc_ptr, owned_flag]`.
/// The owned flag distinguishes borrowed props-entry descs (owned=0, the
/// pre-刀-23 shape) from accessor-getter products (owned=1, need a
/// `__torajs_value_drop_heap` when the buffer is released).
const BUF_STRIDE_U64: usize = 3;
const BUF_STRIDE_BYTES: usize = BUF_STRIDE_U64 * 8;

/// Release the Phase-1 buffer, dropping any owned getter products still
/// parked in it. Every error / normal exit routes through here so the
/// owned-vs-borrowed accounting cannot drift.
unsafe fn release_buf(buf: *mut u64, buf_bytes: usize, n: usize) {
    for j in 0..n {
        if unsafe { *buf.add(j * BUF_STRIDE_U64 + 2) } != 0 {
            let desc = unsafe { *buf.add(j * BUF_STRIDE_U64 + 1) } as *mut c_void;
            if !desc.is_null() {
                unsafe { __torajs_value_drop_heap(desc) };
            }
        }
    }
    unsafe { free(buf as *mut c_void, buf_bytes) };
}

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
    // Spec §20.1.2.3.1 ObjectDefineProperties walks `props`'s
    // OwnPropertyKeys — every JS object shape qualifies, not just
    // DynObj. Function / Array / Number / String / Boolean wrapper
    // cells own an expando dynobj slot; walk that when present. A
    // NULL expando (no `props.foo = ...` ever written) iterates zero
    // enumerable own keys — spec-valid: `Object.create({}, function(){})`
    // returns `obj` untouched. TAG_OBJ (static-layout struct) walks
    // its layout via `define_all_from_struct`.
    let source_tag = unsafe { type_tag(props) };
    let props: *const c_void = match source_tag {
        TAG_DYNOBJ => props,
        TAG_CLOSURE_HDR | TAG_ARR_HDR => {
            let expando =
                unsafe { *(props.cast::<u8>().add(CELL_PROPS_OFF) as *const *const c_void) };
            if expando.is_null() {
                return;
            }
            expando
        }
        TAG_NUMBER_WRAPPER | TAG_STRING_WRAPPER | TAG_BOOLEAN_WRAPPER => {
            let expando =
                unsafe { *(props.cast::<u8>().add(WRAPPER_PROPS_OFF) as *const *const c_void) };
            if expando.is_null() {
                return;
            }
            expando
        }
        TAG_OBJ => {
            // Static-layout struct container — its own enumerable
            // walk is the layout, not a dynobj entry table.
            return unsafe { define_all_from_struct(obj_slot, props) };
        }
        _ => {
            unsafe {
                __torajs_throw_type_error(
                    c"Property description must be an object.".as_ptr() as *const u8
                );
            }
            return;
        }
    };
    let len = unsafe { entries_len(props) } as usize;
    if len == 0 {
        return;
    }
    // Phase 1 — collect (key, desc, owned) triples for every enumerable
    // own entry, validating each descriptor is an object.
    let buf_bytes = len * BUF_STRIDE_BYTES;
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
        let raw_tag = unsafe { __torajs_anyv_unbox_tag(desc_anyv) } as u64;
        let raw_val = unsafe { __torajs_anyv_unbox_value(desc_anyv) } as u64;
        // RFC 20260716 刀 23 — spec §20.1.2.3.1 step 3.b [[Get]](props,
        // key): an accessor own entry (raw value is `AccessorPair`)
        // invokes its getter (spec §10.1.8.1) and routes the OWNED
        // result through the accept gate. A pair with no getter
        // yields `undefined`; a throwing getter surfaces as a pending
        // throw here — both propagate correctly through the standard
        // (tag != HEAP) reject path and pending-throw exit below.
        let (d_tag, d_val, owned) = if raw_tag == ANY_HEAP
            && raw_val != 0
            && unsafe { type_tag(raw_val as *const c_void) } == TAG_ACCESSOR_PAIR
        {
            let g = unsafe { __torajs_accessor_invoke_getter(raw_val as *const c_void) };
            if unsafe { __torajs_throw_check() } != 0 {
                // Getter threw — invoke-getter never plants a live ref
                // on the throw exit, nothing new to drop; release the
                // already-collected buffer (owned entries drop) and
                // propagate.
                unsafe { release_buf(buf, buf_bytes, n) };
                return;
            }
            (
                unsafe { __torajs_anyv_unbox_tag(g) } as u64,
                unsafe { __torajs_anyv_unbox_value(g) } as u64,
                true,
            )
        } else {
            (raw_tag, raw_val, false)
        };
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
            // Reject path — if the accessor getter returned a heap
            // value that failed the accept gate (e.g. primitive Str),
            // its +1 rc must be released here since it never lands in
            // the buffer that `release_buf` walks.
            if owned && d_tag == ANY_HEAP && d_val != 0 {
                unsafe { __torajs_value_drop_heap(d_val as *mut c_void) };
            }
            unsafe {
                __torajs_throw_type_error(
                    c"Property description must be an object.".as_ptr() as *const u8
                );
                release_buf(buf, buf_bytes, n);
            }
            return;
        }
        unsafe {
            *buf.add(n * BUF_STRIDE_U64) = bucket_key_ptr(kp_tagged) as u64;
            *buf.add(n * BUF_STRIDE_U64 + 1) = d_val;
            *buf.add(n * BUF_STRIDE_U64 + 2) = owned as u64;
        }
        n += 1;
    }
    // Phase 2 — apply in collection order; stop at the first pending
    // throw (non-configurable redefine / bad accessor field).
    // `define_from_desc` borrows `desc` (no rc consumed), so owned
    // getter products still need `release_buf` to drop their +1 rc
    // whether Phase 2 ran them or broke out early.
    for j in 0..n {
        let key = unsafe { *buf.add(j * BUF_STRIDE_U64) } as *mut c_void;
        let desc = unsafe { *buf.add(j * BUF_STRIDE_U64 + 1) } as *const c_void;
        unsafe { __torajs_dynobj_define_from_desc(obj_slot, key, desc) };
        if unsafe { __torajs_throw_check() } != 0 {
            break;
        }
    }
    unsafe { release_buf(buf, buf_bytes, n) };
}

/// Release the minted layout-keys array (owned Str elements; the arr
/// elem kind is UNSET so its drop can't cascade — obj_assign twin).
unsafe fn release_struct_keys(keys: *mut c_void, len: usize, data: *const u64) {
    for i in 0..len {
        let key = unsafe { data.add(i).read() } as *mut c_void;
        if !key.is_null() {
            unsafe { __torajs_value_drop_heap(key) };
        }
    }
    unsafe { __torajs_value_drop_heap(keys) };
}

/// `TAG_OBJ` (static-layout struct) `props` container — §20.1.2.3.1
/// over layout fields. Same two-phase shape as the dynobj walk (every
/// descriptor validates before the first apply; a getter field runs
/// once via [[Get]]): Phase 1 probes each layout key through the
/// anyvalue member channel (borrow-shaped; accessor sentinel routes
/// through `any_accessor_get`, owned) into the shared triple buffer,
/// Phase 2 applies via `define_from_desc` in field order.
unsafe fn define_all_from_struct(obj_slot: *mut *mut c_void, props: *const c_void) {
    let props_anyv = unsafe { __torajs_anyv_box_from_pair(ANY_HEAP as i64, props as i64) };
    let keys = unsafe { __torajs_anyv_struct_keys(props_anyv) };
    let klen = unsafe { (keys.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() } as usize;
    let kdata = unsafe { (keys.cast::<u8>().add(ARR_DATA_PTR_OFF) as *const *const u64).read() };
    if klen == 0 {
        return unsafe { release_struct_keys(keys, klen, kdata) };
    }
    let buf_bytes = klen * BUF_STRIDE_BYTES;
    let buf = unsafe { calloc(buf_bytes) } as *mut u64;
    let mut n = 0usize;
    for i in 0..klen {
        let key = unsafe { kdata.add(i).read() } as *mut c_void;
        if key.is_null() {
            continue;
        }
        let tag = unsafe { __torajs_any_member_get_tag(props_anyv, key) };
        let (d_tag, d_val, owned) = if tag == ANY_ACCESSOR_TAG {
            let pair_bits = unsafe { __torajs_any_member_get_value(props_anyv, key) };
            let g = unsafe { __torajs_any_accessor_get(props_anyv, key, pair_bits) };
            if unsafe { __torajs_throw_check() } != 0 {
                unsafe { release_buf(buf, buf_bytes, n) };
                return unsafe { release_struct_keys(keys, klen, kdata) };
            }
            let gt = unsafe { __torajs_anyv_unbox_tag(g) } as u64;
            let gv = unsafe { __torajs_anyv_unbox_value(g) } as u64;
            (gt, gv, true)
        } else {
            let v = unsafe { __torajs_any_member_get_value(props_anyv, key) };
            (tag, v, false)
        };
        // Accept gate — mirror of the dynobj walk's desc_ok above.
        let desc_ok = d_tag == ANY_HEAP
            && d_val != 0
            && matches!(
                unsafe { type_tag(d_val as *const c_void) },
                TAG_DYNOBJ | TAG_CLOSURE_HDR | TAG_ARR_HDR | TAG_OBJ
            );
        if !desc_ok {
            if owned && d_tag == ANY_HEAP && d_val != 0 {
                unsafe { __torajs_value_drop_heap(d_val as *mut c_void) };
            }
            unsafe {
                __torajs_throw_type_error(
                    c"Property description must be an object.".as_ptr() as *const u8
                );
                release_buf(buf, buf_bytes, n);
            }
            return unsafe { release_struct_keys(keys, klen, kdata) };
        }
        unsafe {
            *buf.add(n * BUF_STRIDE_U64) = key as u64;
            *buf.add(n * BUF_STRIDE_U64 + 1) = d_val;
            *buf.add(n * BUF_STRIDE_U64 + 2) = owned as u64;
        }
        n += 1;
    }
    for j in 0..n {
        let key = unsafe { *buf.add(j * BUF_STRIDE_U64) } as *mut c_void;
        let desc = unsafe { *buf.add(j * BUF_STRIDE_U64 + 1) } as *const c_void;
        unsafe { __torajs_dynobj_define_from_desc(obj_slot, key, desc) };
        if unsafe { __torajs_throw_check() } != 0 {
            break;
        }
    }
    unsafe { release_buf(buf, buf_bytes, n) };
    unsafe { release_struct_keys(keys, klen, kdata) };
}
