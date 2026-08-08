//! `JSON.parse(text, reviver)` — §25.5.1 steps 7-10 (RFC
//! 20260808-json-parse-any blade 3).
//!
//! Rides the blade-1 kernel for the parse itself, then walks the
//! value tree bottom-up per §25.5.1.1 InternalizeJSONProperty: array
//! elements by ascending index, object entries in the ES own-key
//! visit order (`__torajs_dynobj_iter_order` — the parse never mints
//! non-string keys, but numeric spellings sort first), children
//! before the holder. Each visited property calls
//! `reviver.call(holder, name, val)` through the uniform boxed ABI
//! (`invoke_with_this` — a FLAG_CLOSURE_RECV_FIRST reviver receives
//! the holder as `this`); an `undefined` answer deletes the property
//! (array slots approximate the spec's delete-into-hole as an
//! undefined store — torajs arrays are dense), any other answer
//! replaces it.
//!
//! A non-callable reviver returns the unfiltered root per step 7's
//! IsCallable gate. A throw anywhere (reviver body, getter-free here
//! so only the reviver) unwinds the walk, releasing owned
//! intermediates; the compiler's throw_check after the call
//! propagates it.
//!
//! Approximation (doc'd, not silent): a reviver that DELETES a
//! sibling entry mid-walk leaves a hole; the snapshot walk skips it
//! instead of re-observing `undefined` through a fresh Get.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::method_call_closure_dispatch::{closure_boxed_entry, invoke_with_this};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_undefined};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;

/// `nanbox_encode` pair-encoding heap tag.
const ANY_HEAP: u64 = 4;
/// torajs-arr layout mirror (asserted there).
const ARR_LEN_OFF: usize = 8;

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;

    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);

    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    fn __torajs_dynobj_delete(obj: *mut c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_value(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;

    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_set_any(arr: *mut c_void, i: u64, tag: u64, value: u64);
}

/// `JSON.parse(text, reviver)` — parse + internalize. Returns the
/// OWNED result, or `undefined` with a pending throw recorded.
///
/// # Safety
/// `text` / `reviver` carry valid AnyValue bit patterns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_parse_reviver(
    text: AnyValue,
    reviver: AnyValue,
) -> AnyValue {
    unsafe {
        let root = crate::json_any::__torajs_json_parse_any(text);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        // §25.5.1 step 7 — IsCallable(reviver) false: unfiltered.
        let Some((cell, entry)) = closure_boxed_entry(reviver) else {
            return root;
        };
        // Steps 8-10 — root holder `{ "": root }`; root ownership
        // transfers into the holder entry.
        let mut holder = __torajs_dynobj_alloc();
        let k_root = __torajs_str_alloc(b"".as_ptr(), 0);
        let (t, v) = (
            crate::__torajs_anyv_unbox_tag(root),
            crate::__torajs_anyv_unbox_value(root),
        );
        __torajs_dynobj_set(&mut holder, k_root.cast(), t as u64, v as u64);
        let holder_av = __torajs_anyv_box_from_pair(ANY_HEAP as i64, holder as i64);
        let val = __torajs_dynobj_iter_value(holder, 0);
        let out = internalize(holder_av, k_root.cast(), val, cell, entry);
        __torajs_str_drop(k_root.cast());
        crate::nanbox_ffi::__torajs_anyv_rc_dec(holder_av);
        if __torajs_throw_check() != 0 {
            release(out);
            return VALUE_UNDEFINED;
        }
        out
    }
}

unsafe fn release(v: AnyValue) {
    unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(v) }
}

/// §25.5.1.1 InternalizeJSONProperty — `val` is the BORROWED current
/// value of `holder[name]`; children internalize in place, then the
/// reviver answers the OWNED replacement for this property.
unsafe fn internalize(
    holder_av: AnyValue,
    name_key: *mut c_void,
    val: AnyValue,
    cell: *mut c_void,
    entry: u64,
) -> AnyValue {
    unsafe {
        if is_cell(val) {
            let ptr = as_void_ptr(val);
            let type_tag = (ptr.cast::<u8>().add(4) as *const u16).read();
            if type_tag == Tag::Arr as u16 {
                internalize_array_children(val, ptr, cell, entry);
            } else if type_tag == Tag::DynObj as u16 {
                internalize_object_children(val, ptr, cell, entry);
            }
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
        }
        // Call(reviver, holder, « name, val ») — argv is borrowed,
        // the answer is owned.
        let name_boxed = __torajs_anyv_box_from_pair(ANY_HEAP as i64, name_key as i64);
        let argv = [name_boxed, val];
        invoke_with_this(cell, entry, holder_av, argv.as_ptr(), 2)
    }
}

unsafe fn internalize_array_children(
    val_av: AnyValue,
    arr: *mut c_void,
    cell: *mut c_void,
    entry: u64,
) {
    unsafe {
        let len = (arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
        for i in 0..len {
            let child = __torajs_arr_get_any_boxed(arr, i);
            let key = index_key(i);
            let r = internalize(val_av, key.cast(), child, cell, entry);
            __torajs_str_drop(key.cast());
            if __torajs_throw_check() != 0 {
                release(r);
                return;
            }
            // Dense-array approximation of the spec's delete: an
            // undefined answer stores undefined (5, 0) in the slot.
            let (t, v) = (
                crate::__torajs_anyv_unbox_tag(r),
                crate::__torajs_anyv_unbox_value(r),
            );
            __torajs_arr_set_any(arr, i, t as u64, v as u64);
        }
    }
}

unsafe fn internalize_object_children(
    val_av: AnyValue,
    obj: *mut c_void,
    cell: *mut c_void,
    entry: u64,
) {
    unsafe {
        let len = __torajs_dynobj_iter_len(obj);
        if len == 0 {
            return;
        }
        let mut order = vec![0u64; len as usize];
        let n = __torajs_dynobj_iter_order(obj, order.as_mut_ptr(), len);
        for &idx in order.iter().take(n as usize) {
            let key = __torajs_dynobj_iter_key(obj, idx);
            if key.is_null() {
                // deleted by an earlier reviver call — snapshot skip
                continue;
            }
            // The reviver may delete THIS entry too; keep the key
            // cell alive across the call.
            crate::payload_rc_inc(ANY_HEAP as i64, key as i64);
            let child = __torajs_dynobj_iter_value(obj, idx);
            let r = internalize(val_av, key, child, cell, entry);
            if __torajs_throw_check() != 0 {
                release(r);
                __torajs_str_drop(key);
                return;
            }
            if is_undefined(r) {
                __torajs_dynobj_delete(obj, key);
            } else {
                let (t, v) = (
                    crate::__torajs_anyv_unbox_tag(r),
                    crate::__torajs_anyv_unbox_value(r),
                );
                let mut slot = obj;
                __torajs_dynobj_set(&mut slot, key, t as u64, v as u64);
            }
            __torajs_str_drop(key);
        }
    }
}

/// Mint the decimal Str key for array index `i` (reviver `name`
/// argument — §25.5.1.1 walks arrays with string-typed keys).
unsafe fn index_key(i: u64) -> *mut u8 {
    let mut buf = [0u8; 20];
    let mut n = i;
    let mut end = buf.len();
    loop {
        end -= 1;
        buf[end] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    unsafe { __torajs_str_alloc(buf[end..].as_ptr(), (buf.len() - end) as i64) }
}
