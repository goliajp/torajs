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

mod ops;

use torajs_rc::{
    ANY_METHOD_COPY_WITHIN, ANY_METHOD_FILL, ANY_METHOD_POP, ANY_METHOD_PUSH, ANY_METHOD_REVERSE,
    ANY_METHOD_SHIFT, ANY_METHOD_SORT, ANY_METHOD_SPLICE, ANY_METHOD_UNSHIFT,
};

use crate::method_call::{closure_boxed_entry, not_callable, to_index};
use crate::method_call_arraylike::{arraylike_get, arraylike_has};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_undefined};
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
    /// torajs-arr — fresh Array<Any> (splice's removed product /
    /// sort's staging).
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    /// torajs-arr — append one (tag, value) pair; ANY_HEAP transfers
    /// ownership. Caller must capture the (possibly moved) return.
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    /// torajs-arr — in-place stable merge sort over an Array<Any>
    /// (boxed comparator or the §23.1.3.30.2 ToString default).
    fn __torajs_arr_any_sort(
        arr: *mut u8,
        cb_env: *mut c_void,
        cb_entry: u64,
        has_cb: i64,
    ) -> *mut u8;
    /// torajs-arr — borrowed whole-box slot read (the sort
    /// writeback).
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    /// torajs-throw — pending-throw flag (the sort comparator leg).
    fn __torajs_throw_check() -> i64;
    /// torajs-throw — §23.1.3.23/.31/.36 integer-limit gates (records
    /// the pending throw and returns; the call site's throw-check
    /// propagates).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_range_error(msg: *const core::ffi::c_char);
}

/// 3b mutator set — the dynobj routing gates on this alongside
/// the read family.
pub(crate) fn arraylike_mut_supported(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_POP
            | ANY_METHOD_PUSH
            | ANY_METHOD_SHIFT
            | ANY_METHOD_UNSHIFT
            | ANY_METHOD_REVERSE
            | ANY_METHOD_SPLICE
            | ANY_METHOD_SORT
            | ANY_METHOD_FILL
            | ANY_METHOD_COPY_WITHIN
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
        set_prop(
            obj,
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
        set_prop(obj, key as *mut c_void, 2, n as u64);
        __torajs_str_drop(key as *mut c_void);
    }
}

/// Receiver-dispatched `Set` (刀 8 G2a) — a DynObj receiver takes
/// the raw bucket store (resize relocation threads through `obj`);
/// anything else (a struct receiver reached via the call/apply
/// re-dispatch) goes through the member-set dispatcher, where a
/// same-typed field write lands and growth rejects loud (the G10
/// posture) instead of corrupting a non-dynobj layout with a raw
/// bucket store. The pair transfers either way.
unsafe fn set_prop(obj: &mut *mut c_void, key: *mut c_void, tag: u64, value: u64) {
    unsafe {
        let hdr = ((*obj).cast::<u8>().add(4) as *const u16).read();
        if hdr == torajs_rc::Tag::DynObj as u16 {
            __torajs_dynobj_set(obj as *mut *mut c_void, key, tag, value);
            return;
        }
        let mut slot: AnyValue = __torajs_anyv_box_pointer(*obj);
        crate::member_set::__torajs_any_member_set(&mut slot, key, tag, value, -1);
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
                // §23.1.3.23 step 4 — the product length caps at
                // 2^53-1 BEFORE any element write (O(1) early exit;
                // JSC words it per-method).
                if len + argc > 9007199254740991 {
                    __torajs_throw_type_error(
                        c"push cannot produce an array of length larger than (2 ** 53) - 1"
                            .as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
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
                    // §23.1.3.36 step 4.a — same 2^53-1 cap, checked
                    // before the O(len) shift walk (the zero-arg form
                    // never throws).
                    if len + argc > 9007199254740991 {
                        __torajs_throw_type_error(
                            c"unshift cannot produce an array of length larger than (2 ** 53) - 1"
                                .as_ptr(),
                        );
                        return VALUE_UNDEFINED;
                    }
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
            m if m == ANY_METHOD_SPLICE => do_splice(&mut obj, len, argv, argc),
            m if m == ANY_METHOD_SORT => ops::do_sort(&mut obj, len, arg_at(0)),
            m if m == ANY_METHOD_FILL => ops::do_fill(&mut obj, len, argv, argc),
            m if m == ANY_METHOD_COPY_WITHIN => ops::do_copy_within(&mut obj, len, argv, argc),
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

/// `splice(start, deleteCount, …items)` per §23.1.3.31 — the
/// removed Gets transfer into a fresh Array<Any> (absent keys ride
/// the dense-emulation undefined), the gap moves are Has-gated
/// Set/Delete pairs, items store at actualStart, length Sets last.
unsafe fn do_splice(obj: &mut *mut c_void, len: i64, argv: *const u64, argc: i64) -> AnyValue {
    unsafe {
        let arg_at = |i: i64| -> u64 {
            if i < argc {
                *argv.add(i as usize)
            } else {
                VALUE_UNDEFINED
            }
        };
        let rel = to_index(arg_at(0), 0);
        let start = if rel < 0 {
            (rel + len).max(0)
        } else {
            rel.min(len)
        };
        let del = if argc == 0 {
            0
        } else if argc == 1 {
            len - start
        } else {
            to_index(arg_at(1), 0).clamp(0, len - start)
        };
        let items = (argc - 2).max(0);
        // §23.1.3.31 step 8 — the post-splice length caps at 2^53-1
        // (TypeError), then step 9's ArraySpeciesCreate(O,
        // actualDeleteCount) caps the removed product at 2^32-1
        // (RangeError). Both fire before any element read.
        if len + items - del > 9007199254740991 {
            __torajs_throw_type_error(
                c"Splice cannot produce an array of length larger than (2 ** 53) - 1".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        if del > 4294967295 {
            __torajs_throw_range_error(c"Length exceeded the maximum array length".as_ptr());
            return VALUE_UNDEFINED;
        }
        let mut removed = __torajs_arr_alloc_any(del.clamp(0, 4096) as u64);
        for k in 0..del {
            let v = if arraylike_has(*obj, start + k) {
                arraylike_get(*obj, start + k)
            } else {
                VALUE_UNDEFINED
            };
            removed = __torajs_arr_push_any(
                removed as *mut c_void,
                __torajs_anyv_unbox_tag(v) as u64,
                __torajs_anyv_unbox_value(v) as u64,
            );
        }
        if items < del {
            let mut k = start;
            while k < len - del {
                let from = k + del;
                if arraylike_has(*obj, from) {
                    let v = arraylike_get(*obj, from);
                    set_at(obj, k + items, v);
                } else {
                    delete_at(*obj, k + items);
                }
                k += 1;
            }
            let mut k = len;
            while k > len - del + items {
                delete_at(*obj, k - 1);
                k -= 1;
            }
        } else if items > del {
            let mut k = len - del;
            while k > start {
                let from = k + del - 1;
                if arraylike_has(*obj, from) {
                    let v = arraylike_get(*obj, from);
                    set_at(obj, k + items - 1, v);
                } else {
                    delete_at(*obj, k + items - 1);
                }
                k -= 1;
            }
        }
        for i in 0..items {
            let v = arg_at(2 + i);
            __torajs_rc_inc(v as *mut c_void);
            set_at(obj, start + i, v);
        }
        set_len(obj, len - del + items);
        __torajs_anyv_box_pointer(removed as *mut c_void)
    }
}
