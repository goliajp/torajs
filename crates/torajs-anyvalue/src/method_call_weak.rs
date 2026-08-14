//! `Tag::WeakMap` / `Tag::WeakSet` / `Tag::WeakRef` arms of
//! `__torajs_any_method_call`
//! (RFC 20260706-test262-bug-corpus RC-2b) — split out of
//! `method_call.rs` by the 500-line file discipline, mirroring the
//! `method_call_mapset` sibling.
//!
//! Routes onto the torajs-weak kernels (ptr-keyed, values held with
//! rc semantics):
//!
//! - WeakMap: get / set / has / delete. WeakSet: add / has / delete.
//!   Methods of the other kind fall to the catchable TypeError.
//! - Keys must be heap cells ("held weakly", ES §24.3/§24.4): a
//!   primitive key reads as absent (`has` → false, `delete` → false,
//!   `get` → undefined) and THROWS for `set` / `add` (spec
//!   "Invalid value used as weak map key").
//! - `set` values ride the kernel's AnyValue lane (RC-4 F2): cells
//!   get a strong rc from the kernel, primitives are stored
//!   verbatim — spec allows any value, only keys must be cells.
//! - `set` / `add` return `this` (+1, boxed-value convention);
//!   `get` boxes the kernel's already-inc'd hit or `undefined`.

use core::ffi::c_void;

use torajs_rc::any_method_iter::ANY_METHOD_DEREF;
use torajs_rc::{
    __torajs_rc_inc, ANY_METHOD_ADD, ANY_METHOD_DELETE, ANY_METHOD_GET, ANY_METHOD_GET_OR_INSERT,
    ANY_METHOD_GET_OR_INSERT_COMPUTED, ANY_METHOD_HAS, ANY_METHOD_SET,
};

use crate::method_call::method_no_such;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;

unsafe extern "C" {
    /// torajs-weak — ptr-keyed kernels. Keys are held weakly (no
    /// rc); values are NaN-box `AnyValue` bits (RC-4 F2 — cells
    /// strong-rc'd by the kernel, primitives verbatim). `set` incs
    /// the cell value it keeps, `get` incs the cell hit it hands
    /// out (miss → the undefined sentinel).
    fn __torajs_weakmap_set(p: *mut c_void, key: *mut c_void, value: u64);
    fn __torajs_weakmap_get(p: *mut c_void, key: *mut c_void) -> u64;
    fn __torajs_weakmap_has(p: *mut c_void, key: *mut c_void) -> i64;
    fn __torajs_weakmap_delete(p: *mut c_void, key: *mut c_void) -> i64;
    fn __torajs_weakset_add(p: *mut c_void, key: *mut c_void);
    fn __torajs_weakset_has(p: *mut c_void, key: *mut c_void) -> i64;
    fn __torajs_weakset_delete(p: *mut c_void, key: *mut c_void) -> i64;
    /// torajs-weak key classification (ES CanBeHeldWeakly) — NULL
    /// for an illegal key (primitives INCLUDING Str/BigInt cells).
    fn __torajs_weak_key_from_any(av: u64) -> *mut c_void;
    /// torajs-weak — §26.1.4.2 deref in the AnyValue lane.
    fn __torajs_weakref_deref_any(p: *mut c_void) -> u64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Dispatch a method id on a `Tag::WeakMap` / `Tag::WeakSet` cell.
///
/// # Safety
/// `ptr` is a live WeakMap/WeakSet cell; `argv` points at `argc`
/// AnyValue slots the caller keeps alive across the call (BORROWED).
pub(crate) unsafe fn weak_method(
    ptr: *mut c_void,
    is_weakset: bool,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let arg_at = |i: i64| -> u64 {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    // RC-4 F2 — classification via the shared torajs-weak helper:
    // a legal weak key is an object-like cell; Str / BigInt cells
    // back JS primitives and read as illegal (NULL) exactly like a
    // bare number.
    let key_av = arg_at(0);
    let key: *mut c_void = unsafe { __torajs_weak_key_from_any(key_av) };
    unsafe {
        match mid {
            m if m == ANY_METHOD_HAS => {
                let hit = if key.is_null() {
                    0
                } else if is_weakset {
                    __torajs_weakset_has(ptr, key)
                } else {
                    __torajs_weakmap_has(ptr, key)
                };
                __torajs_anyv_box_from_pair(1, hit)
            }
            m if m == ANY_METHOD_DELETE => {
                let hit = if key.is_null() {
                    0
                } else if is_weakset {
                    __torajs_weakset_delete(ptr, key)
                } else {
                    __torajs_weakmap_delete(ptr, key)
                };
                __torajs_anyv_box_from_pair(1, hit)
            }
            m if m == ANY_METHOD_GET && !is_weakset => {
                if key.is_null() {
                    return VALUE_UNDEFINED;
                }
                // Kernel returns AnyValue bits directly — cells come
                // with a +1 the caller owns, miss is the undefined
                // sentinel. No re-boxing needed.
                __torajs_weakmap_get(ptr, key)
            }
            m if (m == ANY_METHOD_GET_OR_INSERT || m == ANY_METHOD_GET_OR_INSERT_COMPUTED)
                && !is_weakset =>
            {
                if key.is_null() {
                    __torajs_throw_type_error(c"Invalid value used as weak map key".as_ptr());
                    return VALUE_UNDEFINED;
                }
                weakmap_upsert(ptr, key, arg_at(1), m == ANY_METHOD_GET_OR_INSERT_COMPUTED)
            }
            m if (m == ANY_METHOD_SET && !is_weakset) || (m == ANY_METHOD_ADD && is_weakset) => {
                if key.is_null() {
                    __torajs_throw_type_error(c"Invalid value used as weak map key".as_ptr());
                    return VALUE_UNDEFINED;
                }
                if is_weakset {
                    __torajs_weakset_add(ptr, key);
                } else {
                    // RC-4 F2 — the kernel value slot is an AnyValue
                    // lane: pass the borrowed bits verbatim (the
                    // kernel incs cells it keeps; primitives ride
                    // free, per spec any value is legal).
                    __torajs_weakmap_set(ptr, key, arg_at(1));
                }
                // set / add return `this` (+1, boxed-value convention).
                __torajs_rc_inc(ptr);
                __torajs_anyv_box_from_pair(4, ptr as i64)
            }
            _ => method_no_such(),
        }
    }
}

/// Dispatch a method id on a `Tag::WeakRef` cell — §26.1.4.2 gives
/// the family exactly one method. The kernel already answers in the
/// AnyValue lane (an alive target's box IS its pointer, +1 rc the
/// caller owns; a cleared one is the undefined sentinel), so the arm
/// is a hand-off.
///
/// # Safety
/// `ptr` is a live WeakRef cell.
pub(crate) unsafe fn weakref_method(ptr: *mut c_void, mid: i64) -> AnyValue {
    unsafe {
        if mid == ANY_METHOD_DEREF {
            return __torajs_weakref_deref_any(ptr);
        }
        method_no_such()
    }
}

/// The stage-3 upsert pair on a WeakMap (383-04) — key already
/// classified non-NULL (ES CanBeHeldWeakly precedes the callable
/// gate in the WeakMap steps, which is the caller split here). The
/// second argument is BORROWED: the plain form's default value, or
/// the computed form's callbackfn. The answer is +1-owned.
///
/// # Safety
/// `ptr` is a live WeakMap cell; `key` a live cell classified by
/// `__torajs_weak_key_from_any`; `arg_av` borrowed bits alive
/// across the call.
unsafe fn weakmap_upsert(
    ptr: *mut c_void,
    key: *mut c_void,
    arg_av: u64,
    computed: bool,
) -> AnyValue {
    unsafe {
        let cb = if computed {
            let Some(pair) = crate::method_call::closure_boxed_entry(arg_av) else {
                __torajs_throw_type_error(c"callbackfn is not a function".as_ptr());
                return VALUE_UNDEFINED;
            };
            Some(pair)
        } else {
            None
        };
        if __torajs_weakmap_has(ptr, key) != 0 {
            // Kernel hit comes +1-owned (the `get` contract above).
            return __torajs_weakmap_get(ptr, key);
        }
        let v = match cb {
            Some((cb_env, cb_entry)) => {
                // A weak key is a cell, so its AnyValue box is its
                // pointer bits — a borrow our caller's argv covers.
                let r = crate::method_call_upsert::call_cb1(cb_env, cb_entry, key as u64);
                if __torajs_throw_check() != 0 {
                    __torajs_value_drop_heap(r as *mut c_void);
                    return VALUE_UNDEFINED;
                }
                r
            }
            None => {
                // Borrowed default — own a return stake on a cell.
                let cp = crate::nanbox_encode::__torajs_anyv_cell_ptr(arg_av);
                if cp != 0 {
                    __torajs_rc_inc(cp as *mut c_void);
                }
                arg_av
            }
        };
        // The kernel incs the cells it keeps; our own stays ours.
        __torajs_weakmap_set(ptr, key, v);
        v
    }
}

/// Typed-lane kernels — key arrives pre-classified by the lowering's
/// `lower_weak_key` (NULL = illegal key; the helper already recorded
/// the pending TypeError for this setter-shaped method, so the
/// kernel only bows out).
///
/// # Safety
/// As [`weakmap_upsert`]; `p` may be null-keyed per above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakmap_get_or_insert(
    p: *mut c_void,
    key: *mut c_void,
    dflt_av: u64,
) -> u64 {
    if key.is_null() {
        return VALUE_UNDEFINED;
    }
    unsafe { weakmap_upsert(p, key, dflt_av, false) }
}

/// # Safety
/// As [`__torajs_weakmap_get_or_insert`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakmap_get_or_insert_computed(
    p: *mut c_void,
    key: *mut c_void,
    cb_av: u64,
) -> u64 {
    if key.is_null() {
        return VALUE_UNDEFINED;
    }
    unsafe { weakmap_upsert(p, key, cb_av, true) }
}
