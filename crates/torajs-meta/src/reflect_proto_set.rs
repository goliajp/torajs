//! `Object.setPrototypeOf(O, proto)` + the `o.__proto__ = v` setter
//! (Annex B §B.2.2.1) — RFC 20260717-user-proto-chain knife 3.
//!
//! Both faces share §10.1.2.1 OrdinarySetPrototypeOf over the
//! `__proto__` simulation slot: same-value short-circuit, the
//! non-extensible refusal, the chain cycle walk (following the same
//! user-chain steps the knife-2 read path takes; a non-dynobj link
//! ends the walk — its implicit builtin chain cannot loop back into
//! a user object), then the write (a null proto deletes the entry
//! and sets the null-proto header bit; a cell clears the bit and
//! lands in the slot with its own +1).
//!
//! The two faces differ only in failure reporting, per spec:
//! `Object.setPrototypeOf` throws TypeError on an invalid proto arg
//! and on OrdinarySetPrototypeOf refusing (cycle / non-extensible);
//! the `__proto__` setter silently ignores an invalid value and
//! throws only on the refusal.
//!
//! Recorded boundary: non-dynobj receivers (Arr / Closure / wrapper
//! / struct cells) keep their builtin [[Prototype]] — the write is
//! ignored (spec would allow re-parenting exotic objects; the
//! per-shape proto slot is a follow-up knife).

use core::ffi::{c_char, c_void};

use crate::reflect::{
    ANY_HEAP, DYNOBJ_HDR_FLAG_NULL_PROTO, TAG_DYNOBJ, VALUE_NULL_IMM, alloc_str_key, heap_type_tag,
    is_cell_imm,
};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const u8) -> bool;
    fn __torajs_dynobj_get_tag(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    fn __torajs_dynobj_delete(obj: *mut c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_mark_null_proto(obj: *mut c_void);
    fn __torajs_dynobj_clear_null_proto(obj: *mut c_void);
    fn __torajs_throw_type_error(msg: *const c_char);
}

/// The two null encodings that reach these faces: the canonical
/// `VALUE_NULL_IMM` NaN-box and the SSA typed-Null pack (tag 0,
/// value 0 → raw 0 — the same pair `get_proto_of_any` accepts).
fn is_null_any(v: u64) -> bool {
    v == VALUE_NULL_IMM || v == 0
}

/// `DYNOBJ_HDR_FLAG_NON_EXTENSIBLE` mirror (torajs-dynobj layout).
const HDR_FLAG_NON_EXTENSIBLE: u16 = 1 << 8;

/// The receiver's current user-proto link: the own `__proto__` cell
/// entry, or null for the null-proto shape / an absent entry (the
/// implicit %Object.prototype%).
unsafe fn current_proto(dynobj: *const c_void, key: *const u8) -> u64 {
    let flags = unsafe { dynobj.cast::<u8>().add(6).cast::<u16>().read() };
    if flags & DYNOBJ_HDR_FLAG_NULL_PROTO != 0 {
        return VALUE_NULL_IMM;
    }
    if !unsafe { __torajs_dynobj_has(dynobj, key) } {
        return VALUE_NULL_IMM;
    }
    let t = unsafe { __torajs_dynobj_get_tag(dynobj, key) } as i64;
    let v = unsafe { __torajs_dynobj_get_value(dynobj, key) };
    if t == ANY_HEAP && v != 0 {
        v
    } else {
        VALUE_NULL_IMM
    }
}

/// §10.1.2.1 steps 4-8 — refuse on non-extensible (unless same
/// value) and on a chain cycle; write otherwise. `true` = done.
pub(crate) unsafe fn ordinary_set_prototype_of(obj: *mut c_void, proto: u64) -> bool {
    unsafe {
        let proto = if is_null_any(proto) {
            VALUE_NULL_IMM
        } else {
            proto
        };
        let key = alloc_str_key(b"__proto__");
        let cur = current_proto(obj, key);
        // Step 3 — SameValue(V, current) succeeds untouched, even on
        // a non-extensible receiver. The absent-entry implicit
        // %Object.prototype% chain reads as null here; re-linking an
        // implicit chain to an explicit cell is still a write.
        if proto == cur {
            __torajs_str_drop(key);
            return true;
        }
        let flags = obj.cast::<u8>().add(6).cast::<u16>().read();
        if flags & HDR_FLAG_NON_EXTENSIBLE != 0 {
            __torajs_str_drop(key);
            return false;
        }
        // Steps 6-8 — walk the would-be parent chain; finding the
        // receiver itself is the cycle refusal. A non-dynobj link
        // ends the walk (builtin implicit chains cannot reach a
        // user dynobj).
        let mut cur_p = proto;
        while is_cell_imm(cur_p) {
            let cell = cur_p as *mut c_void;
            if core::ptr::eq(cell, obj) {
                __torajs_str_drop(key);
                return false;
            }
            if heap_type_tag(cell) != TAG_DYNOBJ {
                break;
            }
            cur_p = current_proto(cell, key);
        }
        if is_null_any(proto) {
            __torajs_dynobj_delete(obj, key as *const c_void);
            __torajs_dynobj_mark_null_proto(obj);
        } else {
            __torajs_dynobj_clear_null_proto(obj);
            let cell = proto as *mut c_void;
            // The entry owns its reference (dynobj_set transfers the
            // value; overwrite drops the old stake).
            __torajs_rc_inc(cell);
            let mut slot = obj;
            __torajs_dynobj_set(&mut slot, key, ANY_HEAP as u64, cell as u64);
        }
        __torajs_str_drop(key);
        true
    }
}

/// `Object.setPrototypeOf(O, proto)` — §20.1.2.21. Invalid proto is
/// a TypeError; a primitive O passes through untouched (step 3); a
/// refusal (cycle / non-extensible) is a TypeError.
///
/// # Safety
/// `obj` / `proto` carry valid AnyValue bit patterns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_set_prototype_of(obj: u64, proto: u64) {
    unsafe {
        if !is_null_any(proto) && !is_cell_imm(proto) {
            __torajs_throw_type_error(c"Prototype must be an object or null".as_ptr());
            return;
        }
        if !is_cell_imm(obj) {
            return;
        }
        let cell = obj as *mut c_void;
        if heap_type_tag(cell) != TAG_DYNOBJ {
            // Recorded boundary — exotic receivers keep their
            // builtin [[Prototype]].
            return;
        }
        if !ordinary_set_prototype_of(cell, proto) {
            __torajs_throw_type_error(c"Cannot set prototype of this object".as_ptr());
        }
    }
}

/// Annex B §B.2.2.1 `set __proto__(v)` — an invalid v is silently
/// ignored (step 2 returns undefined); only the OrdinarySetPrototypeOf
/// refusal throws.
///
/// # Safety
/// `obj` / `proto` carry valid AnyValue bit patterns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_proto_member_set(obj: u64, proto: u64) {
    unsafe {
        if !is_null_any(proto) && !is_cell_imm(proto) {
            return;
        }
        if !is_cell_imm(obj) {
            return;
        }
        let cell = obj as *mut c_void;
        if heap_type_tag(cell) != TAG_DYNOBJ {
            return;
        }
        if !ordinary_set_prototype_of(cell, proto) {
            __torajs_throw_type_error(c"Cannot set prototype of this object".as_ptr());
        }
    }
}
