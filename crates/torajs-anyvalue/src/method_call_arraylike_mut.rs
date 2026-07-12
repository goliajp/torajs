//! Mutating half of the generic array-like arm (RFC 20260712
//! chunk 3b-1) — pop / push / shift / unshift / reverse over a
//! `Tag::DynObj` receiver, ES §23.1.3.{21,23,29,36,26} generic
//! semantics: per-index Get / Set / DeletePropertyOrThrow +
//! `Set(O, "length", …)` at the end (spec sets length even on the
//! empty fast exits — observable).
//!
//! Relocation: `__torajs_dynobj_set` may move the receiver cell on
//! resize; every Set threads the live pointer, and the (possibly
//! moved) cell writes back through `recv_slot` before returning
//! (NULL slot = the `.call` re-dispatch boundary — same recorded
//! shape as the Arr arm's push-through-call).
//!
//! Argument ledger: argv slots are BORROWED (Sets take their own
//! stakes); per-index Gets are OWNED (transferred into Sets or
//! returned); returns follow the owned boxed-value convention.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_POP, ANY_METHOD_PUSH, ANY_METHOD_REVERSE, ANY_METHOD_SHIFT, ANY_METHOD_UNSHIFT,
};

use crate::method_call_arraylike::{arraylike_get, arraylike_has};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::{
    __torajs_anyv_box_i64, __torajs_anyv_box_pointer, __torajs_anyv_unbox_tag,
    __torajs_anyv_unbox_value,
};

unsafe extern "C" {
    /// torajs-str — fresh Str from raw bytes (index / length keys).
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release a heap Str reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-dynobj — keyed store; resize relocates through the
    /// slot. The (tag, value) pair transfers into the bucket.
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    /// Cross-tier — universal NaN-box-safe heap-value release.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-rc — NaN-box-safe refcount bump (argv borrows →
    /// stored stakes).
    fn __torajs_rc_inc(p: *mut c_void);
}

/// 3b-1 mutator set — the dynobj routing gates on this alongside
/// the read family.
pub(crate) fn arraylike_mut_supported(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_POP
            | ANY_METHOD_PUSH
            | ANY_METHOD_SHIFT
            | ANY_METHOD_UNSHIFT
            | ANY_METHOD_REVERSE
    )
}

/// Decimal key mint shared by the Set / Delete paths.
unsafe fn mint_key(k: i64) -> *mut u8 {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    let mut m = k.max(0) as u64;
    loop {
        i -= 1;
        buf[i] = b'0' + (m % 10) as u8;
        m /= 10;
        if m == 0 {
            break;
        }
    }
    unsafe { __torajs_str_alloc(buf[i..].as_ptr(), (buf.len() - i) as i64) }
}

/// `Set(O, ToString(k), v)` — the OWNED `v`'s stake transfers into
/// the bucket; `obj` threads relocation.
unsafe fn set_at(obj: &mut *mut c_void, k: i64, v: AnyValue) {
    unsafe {
        let key = mint_key(k);
        __torajs_dynobj_set(
            obj as *mut *mut c_void,
            key as *mut c_void,
            __torajs_anyv_unbox_tag(v) as u64,
            __torajs_anyv_unbox_value(v) as u64,
        );
        __torajs_str_drop(key as *mut c_void);
    }
}

/// `Set(O, "length", n)`.
unsafe fn set_len(obj: &mut *mut c_void, n: i64) {
    unsafe {
        let key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6);
        __torajs_dynobj_set(obj as *mut *mut c_void, key as *mut c_void, 2, n as u64);
        __torajs_str_drop(key as *mut c_void);
    }
}

/// `DeletePropertyOrThrow(O, ToString(k))` — dynobj entries are
/// configurable by construction, so the throw leg never fires.
unsafe fn delete_at(obj: *mut c_void, k: i64) {
    unsafe {
        let key = mint_key(k);
        crate::prop_delete::__torajs_any_prop_delete(
            __torajs_anyv_box_pointer(obj),
            key as *const c_void,
        );
        __torajs_str_drop(key as *mut c_void);
    }
}

/// See module doc. `obj` is a live DynObj cell; `len` arrives from
/// the caller's already-run length read.
pub(crate) unsafe fn arraylike_mut(
    obj_init: *mut c_void,
    mid: i64,
    len: i64,
    recv_slot: *mut u64,
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
    let mut obj = obj_init;
    let out = unsafe {
        match mid {
            m if m == ANY_METHOD_POP => {
                if len == 0 {
                    // §23.1.3.22 step 3.b — length is Set even on
                    // the empty exit (observable).
                    set_len(&mut obj, 0);
                    VALUE_UNDEFINED
                } else {
                    let idx = len - 1;
                    let v = arraylike_get(obj, idx);
                    delete_at(obj, idx);
                    set_len(&mut obj, idx);
                    v
                }
            }
            m if m == ANY_METHOD_PUSH => {
                for i in 0..argc {
                    let v = arg_at(i);
                    // Borrowed argv slot → the bucket's stake.
                    __torajs_rc_inc(v as *mut c_void);
                    set_at(&mut obj, len + i, v);
                }
                set_len(&mut obj, len + argc);
                __torajs_anyv_box_i64(len + argc)
            }
            m if m == ANY_METHOD_SHIFT => {
                if len == 0 {
                    set_len(&mut obj, 0);
                    VALUE_UNDEFINED
                } else {
                    let first = arraylike_get(obj, 0);
                    let mut k = 1;
                    while k < len {
                        if arraylike_has(obj, k) {
                            let v = arraylike_get(obj, k);
                            set_at(&mut obj, k - 1, v);
                        } else {
                            delete_at(obj, k - 1);
                        }
                        k += 1;
                    }
                    delete_at(obj, len - 1);
                    set_len(&mut obj, len - 1);
                    first
                }
            }
            m if m == ANY_METHOD_UNSHIFT => {
                if argc > 0 {
                    let mut k = len - 1;
                    while k >= 0 {
                        if arraylike_has(obj, k) {
                            let v = arraylike_get(obj, k);
                            set_at(&mut obj, k + argc, v);
                        } else {
                            delete_at(obj, k + argc);
                        }
                        k -= 1;
                    }
                    for i in 0..argc {
                        let v = arg_at(i);
                        __torajs_rc_inc(v as *mut c_void);
                        set_at(&mut obj, i, v);
                    }
                }
                set_len(&mut obj, len + argc);
                __torajs_anyv_box_i64(len + argc)
            }
            // reverse — §23.1.3.26 two-pointer swap with the four
            // present/absent cases.
            _ => {
                let mut lo = 0;
                let mut hi = len - 1;
                while lo < hi {
                    let lo_has = arraylike_has(obj, lo);
                    let hi_has = arraylike_has(obj, hi);
                    let lo_v = if lo_has {
                        arraylike_get(obj, lo)
                    } else {
                        VALUE_UNDEFINED
                    };
                    let hi_v = if hi_has {
                        arraylike_get(obj, hi)
                    } else {
                        VALUE_UNDEFINED
                    };
                    if hi_has {
                        set_at(&mut obj, lo, hi_v);
                    } else {
                        delete_at(obj, lo);
                    }
                    if lo_has {
                        set_at(&mut obj, hi, lo_v);
                    } else {
                        delete_at(obj, hi);
                    }
                    lo += 1;
                    hi -= 1;
                }
                // The receiver is the return value (chaining) — a
                // fresh stake under the owned protocol.
                __torajs_rc_inc(obj);
                __torajs_anyv_box_pointer(obj)
            }
        }
    };
    if obj != obj_init && !recv_slot.is_null() {
        // Resize relocated the cell — same identity, moved storage;
        // transfer the caller's variable, no rc traffic.
        unsafe { *recv_slot = __torajs_anyv_box_pointer(obj) };
    }
    out
}
