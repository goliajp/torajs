//! DynObj implicit-set — `obj.x = v` + object-literal init.
//!
//! Implements spec §10.1.5.1 OrdinarySet → §10.1.6.2 CreateDataProperty
//! for the "writable=true" path; throws TypeError when overwriting a
//! non-writable entry (`__torajs_throw_type_error` records pending +
//! returns; caller's ssa-lower-side `emit_throw_check` propagates).
//!
//! Fresh inserts append to the dense entry array (insertion order) and
//! point the probed index slot (first tombstone, else first empty) at
//! the new entry: rc-bump the key (entry owns its share) + write
//! default flags (writable / enumerable / configurable all true).
//! Existing entry overwrite: drop the old heap value if ANY_HEAP,
//! preserve the existing flag bits, swap only the NaN-box value.

use core::ffi::c_void;

use crate::accessor::{__torajs_accessor_invoke_setter, value_is_accessor};
use crate::layout::{
    ANY_HEAP, BUCKET_FLAG_WRITABLE, BUCKET_FLAGS_DEFAULT, BUCKET_TAG_MASK,
    DYNOBJ_HDR_FLAG_NON_EXTENSIBLE,
};
use crate::probe::{
    Entry, bucket_flags, bucket_make_key_tagged, count, entries, entries_cap, entries_len,
    index_ptr, key_str_bytes, probe, set_count, set_entries_len,
};
use crate::resize::resize;

unsafe extern "C" {
    /// Cross-tier — torajs-rc's refcount inc. Entry takes ownership
    /// of the key string on fresh insert.
    fn __torajs_rc_inc(p: *mut c_void);

    /// Cross-tier — torajs-throw's TypeError thrower. Records pending
    /// throw via TLS + returns normally; caller MUST explicitly
    /// `return;` after invoking (per `feedback_throw_extern_returns_void`).
    fn __torajs_throw_type_error(msg: *const u8);

    /// Cross-tier — heap-value drop dispatch (NaN-box-safe; immediate
    /// AnyValues are filtered by the 7d-A cell gate). Drops the old
    /// entry value when overwriting.
    fn __torajs_value_drop_heap(child: *mut c_void);

    /// torajs-anyvalue — NaN-box AnyValue pair encoder. Takes (tag,
    /// value) as i64 + returns the packed u64 immediate.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;

    /// torajs-anyvalue — NaN-box AnyValue tag decoder.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;

    /// torajs-rc — builtin-prototype-singleton own-write note (RFC
    /// 20260721 刀 11 G13): marks the (proto tag, mid) patch bit the
    /// any-method dispatcher's primitive fast arms pre-gate on.
    fn __torajs_builtin_proto_note_own_write(obj: *const c_void, name: *const u8, len: i64);
}

/// `__torajs_dynobj_set(obj_slot, key, tag, value)` — implicit-set entry.
///
/// # Safety
/// `obj_slot` is non-NULL and points at a live `*mut c_void` holding
/// a dynobj or NULL. `key` is a live Str heap pointer. Caller must
/// check for pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
) {
    unsafe { dynobj_set_impl(obj_slot, key, tag, value, true) };
}

/// §28.1.13 Reflect.set flavor — a [[Set]] refusal (readonly entry /
/// setter-less accessor / non-extensible fresh key) answers `0`
/// instead of recording the strict-assignment TypeError; a handled
/// write answers `1`. Same R3-style parameterization as
/// `__torajs_any_prop_delete_soft`.
///
/// # Safety
/// Same contract as [`__torajs_dynobj_set`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set_soft(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
) -> i64 {
    unsafe { dynobj_set_impl(obj_slot, key, tag, value, false) }
}

/// Flavor-carrying core of the two shells above — `throw_on_refusal`
/// picks §13.15.2 strict-assignment (record the TypeError) or
/// §28.1.13 Reflect.set (answer false). Both flavors return the
/// success i64 (1 = written / setter ran, 0 = refused).
unsafe fn dynobj_set_impl(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> i64 {
    let mut obj = unsafe { *obj_slot };
    if obj.is_null() {
        return 1;
    }
    // A write landing on a builtin `<Ctor>.prototype` singleton is a
    // monkey-patch — note it for the fast-arm pre-gate (RFC 20260721
    // 刀 11 G13; one relaxed load when no singleton exists). The gate
    // keys off the method NAME, so a symbol key has nothing to report
    // and must not have the Str payload offsets read off its 16-byte
    // cell.
    if let Some((data, len)) = unsafe { key_str_bytes(key) } {
        unsafe { __torajs_builtin_proto_note_own_write(obj, data, len as i64) };
    }
    // Dense-array-full guard: compact (and grow if genuinely full)
    // before probing so a fresh insert always has an append slot.
    if unsafe { entries_len(obj) } == unsafe { entries_cap(obj) } {
        unsafe {
            resize(obj_slot);
            obj = *obj_slot;
        }
    }

    let pr = unsafe { probe(obj, key as *const c_void) };
    let ent = unsafe { entries(obj) };
    if pr.found {
        let e = unsafe { ent.add(pr.entry as usize) };
        let cur_value_anyv = unsafe { (*e).value_anyv };
        // RFC C3 — accessor entry: dispatch the setter, never a data
        // write (checked before the writable gate — accessors carry no
        // writable bit, so the data path would wrongly throw "read
        // only"). `cur_value_anyv` is the AccessorPair cell verbatim.
        if unsafe { value_is_accessor(cur_value_anyv) } {
            let value_anyv = unsafe {
                __torajs_anyv_box_from_pair((tag & BUCKET_TAG_MASK) as i64, value as i64)
            };
            // Receiver = the dynobj itself, boxed — an ACC_KIND_RECV
            // fn-expr face reads it as `this`; other faces ignore it.
            let recv_anyv = unsafe { __torajs_anyv_box_from_pair(4, obj as i64) };
            if unsafe {
                __torajs_accessor_invoke_setter(
                    cur_value_anyv as *const c_void,
                    recv_anyv,
                    value_anyv,
                )
            } == 0
            {
                if throw_on_refusal {
                    unsafe {
                        __torajs_throw_type_error(
                            c"Attempted to assign to readonly property.".as_ptr() as *const u8,
                        );
                    }
                }
                return 0;
            }
            return 1;
        }
        let cur_flags = bucket_flags(unsafe { (*e).key_ptr_tagged });
        if cur_flags & BUCKET_FLAG_WRITABLE == 0 {
            if throw_on_refusal {
                unsafe {
                    __torajs_throw_type_error(
                        c"Attempted to assign to readonly property.".as_ptr() as *const u8,
                    );
                }
            }
            return 0;
        }
        let cur_tag = unsafe { __torajs_anyv_unbox_tag(cur_value_anyv) } as u64;
        // Drop the old heap value if the current slot was ANY_HEAP.
        // (The NaN-box cell gate in __torajs_value_drop_heap would
        // also short-circuit immediates, but checking tag first keeps
        // the hot path one extern call lighter on the common case.)
        if cur_tag == ANY_HEAP {
            unsafe {
                __torajs_value_drop_heap(cur_value_anyv as *mut c_void);
            }
        }
        // Preserve existing flag bits in key_ptr_tagged; rebox the
        // (tag, value) pair into a fresh NaN-box AnyValue.
        unsafe {
            (*e).value_anyv =
                __torajs_anyv_box_from_pair((tag & BUCKET_TAG_MASK) as i64, value as i64);
        }
        1
    } else {
        // §10.1.5.1 [[Set]] on a non-extensible dict must reject new
        // keys in strict mode (tr modules are all strict). Mirrors
        // `__torajs_dynobj_define`'s fresh-insert gate so
        // `Object.preventExtensions(o); o.newKey = 5;` throws with
        // bun-parity wording — implicit-set was previously silently
        // creating the entry (7 test262 Object/preventExtensions/
        // 15.2.3.10-3-{2,5-1,6,7,12,15,16,17}.js).
        let header_flags = unsafe { *(obj.cast::<u8>().add(6) as *const u16) };
        if header_flags & DYNOBJ_HDR_FLAG_NON_EXTENSIBLE != 0 {
            if throw_on_refusal {
                unsafe {
                    __torajs_throw_type_error(
                        c"Attempting to define property on object that is not extensible.".as_ptr()
                            as *const u8,
                    );
                }
            }
            return 0;
        }
        // Fresh insert: append to the dense array (insertion order),
        // point the probed slot (tombstone reuse or empty) at it.
        let e_idx = unsafe { entries_len(obj) };
        unsafe {
            __torajs_rc_inc(key);
            *ent.add(e_idx as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key, BUCKET_FLAGS_DEFAULT),
                value_anyv: __torajs_anyv_box_from_pair(
                    (tag & BUCKET_TAG_MASK) as i64,
                    value as i64,
                ),
            };
            *index_ptr(obj).add(pr.slot as usize) = e_idx;
            set_entries_len(obj, e_idx + 1);
            set_count(obj, count(obj) + 1);
        }
        1
    }
}

/// Raw attribute-flags upsert — RFC 20260712-arr-exotic-define
/// chunk B. The Array DefineOwnProperty kernel (torajs-arr) stores
/// per-index attribute flags as shadow entries in the array's expando
/// dynobj; it has already run the §10.1.6.3 validation, so this
/// bypasses `dynobj_define`'s checks. A hit rewrites only the flag
/// bits in `key_ptr_tagged`; a miss inserts a fresh entry whose value
/// slot is dead (`undefined` — the element storage owns the value).
///
/// # Safety
/// `obj_slot` is non-NULL and points at a live `*mut c_void` holding
/// a dynobj (non-NULL — the caller allocates). `key` is a live Str
/// heap pointer. `flags` uses the `BUCKET_FLAG_*` bit positions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set_entry_flags(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    flags: u64,
) {
    let mut obj = unsafe { *obj_slot };
    if obj.is_null() {
        return;
    }
    if unsafe { entries_len(obj) } == unsafe { entries_cap(obj) } {
        unsafe {
            resize(obj_slot);
            obj = *obj_slot;
        }
    }
    let pr = unsafe { probe(obj, key as *const c_void) };
    let ent = unsafe { entries(obj) };
    if pr.found {
        let e = unsafe { ent.add(pr.entry as usize) };
        let key_ptr =
            (unsafe { (*e).key_ptr_tagged } & crate::layout::BUCKET_KEY_PTR_MASK) as *mut c_void;
        unsafe { (*e).key_ptr_tagged = bucket_make_key_tagged(key_ptr, flags) };
        // A flags upsert revives a hole entry (RFC 20260713 chunk C):
        // the dead value slot resets to the live-shadow spelling.
        unsafe { (*e).value_anyv = __torajs_anyv_box_from_pair(5, 0) };
    } else {
        let e_idx = unsafe { entries_len(obj) };
        unsafe {
            __torajs_rc_inc(key);
            *ent.add(e_idx as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key, flags),
                value_anyv: __torajs_anyv_box_from_pair(5, 0),
            };
            *index_ptr(obj).add(pr.slot as usize) = e_idx;
            set_entries_len(obj, e_idx + 1);
            set_count(obj, count(obj) + 1);
        }
    }
}

/// Shadow-entry HOLE sentinel (RFC 20260713-defprop-tpd-cluster
/// chunk C) — `delete arr[i]` marks the index absent while element
/// storage stays dense. A hole entry is a shadow entry whose dead
/// value slot holds [`crate::layout::DYNOBJ_HOLE_SENTINEL`] instead
/// of the live `VALUE_UNDEFINED`; flag bits read as 0 (w/e/c all
/// false is a distinct, legal live state — the sentinel
/// disambiguates).
///
/// # Safety
/// Same contract as [`__torajs_dynobj_set_entry_flags`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set_entry_hole(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
) {
    let mut obj = unsafe { *obj_slot };
    if obj.is_null() {
        return;
    }
    if unsafe { entries_len(obj) } == unsafe { entries_cap(obj) } {
        unsafe {
            resize(obj_slot);
            obj = *obj_slot;
        }
    }
    let pr = unsafe { probe(obj, key as *const c_void) };
    let ent = unsafe { entries(obj) };
    if pr.found {
        let e = unsafe { ent.add(pr.entry as usize) };
        let key_ptr =
            (unsafe { (*e).key_ptr_tagged } & crate::layout::BUCKET_KEY_PTR_MASK) as *mut c_void;
        unsafe { (*e).key_ptr_tagged = bucket_make_key_tagged(key_ptr, 0) };
        // Drop the entry's current value before the sentinel lands —
        // deleting an accessor index would otherwise leak the pair
        // (RFC 20260713 chunk C; NaN-box-safe, immediates no-op).
        unsafe { __torajs_value_drop_heap((*e).value_anyv as *mut c_void) };
        unsafe { (*e).value_anyv = crate::layout::DYNOBJ_HOLE_SENTINEL };
    } else {
        let e_idx = unsafe { entries_len(obj) };
        unsafe {
            __torajs_rc_inc(key);
            *ent.add(e_idx as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key, 0),
                value_anyv: crate::layout::DYNOBJ_HOLE_SENTINEL,
            };
            *index_ptr(obj).add(pr.slot as usize) = e_idx;
            set_entries_len(obj, e_idx + 1);
            set_count(obj, count(obj) + 1);
        }
    }
}

/// Whether `key`'s entry is a HOLE sentinel (see
/// [`__torajs_dynobj_set_entry_hole`]). Absent key answers 0.
///
/// # Safety
/// `obj` is null or a live dynobj heap pointer; `key` a live Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_entry_is_hole(
    obj: *const c_void,
    key: *const c_void,
) -> i32 {
    if obj.is_null() {
        return 0;
    }
    let pr = unsafe { probe(obj, key) };
    if !pr.found {
        return 0;
    }
    let v = unsafe { (*entries(obj).add(pr.entry as usize)).value_anyv };
    (v == crate::layout::DYNOBJ_HOLE_SENTINEL) as i32
}
