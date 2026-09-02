//! `__torajs_dynobj_iter_print_order` — the order `console.log`
//! walks an object's own properties, which is bun's order rather
//! than §10.1.11.1's.
//!
//! bun prints what JSC's `getPropertyNamesFromStructure` hands it:
//! when no array-index key is present the structure's insertion
//! order is emitted as is, Symbol keys interleaved with string keys
//! (`{ b: 1, [s]: 2, a: 4 }` prints in that order); once an index
//! key exists the walk goes through the butterfly first and the
//! symbols are collected after the strings (`{ "1": 7, x, y, [s] }`).
//! `Reflect.ownKeys` keeps the spec's three buckets either way — this
//! order is for the printer alone. An array's expando face always
//! comes after its elements, so it takes the second shape whether or
//! not the face itself holds an index key
//! ([`__torajs_dynobj_iter_print_order_after_index`]).

use core::ffi::c_void;

use crate::get::type_tag;
use crate::iter::{__torajs_dynobj_iter_order, key_array_index};
use crate::layout::{DYNOBJ_KEY_HOLE, TAG_DYNOBJ};
use crate::probe::{bucket_key_ptr, entries, entries_len, key_is_symbol};

/// Fill `out[0..n]` with dense-entry indices in print order and
/// return `n`. Same contract as [`__torajs_dynobj_iter_order`]:
/// `cap` must be ≥ `iter_len(obj)`; NULL / foreign-tag / short-cap
/// inputs answer 0. Holes are pre-excluded.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header;
/// `out` is null or valid for `cap` u64 writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_iter_print_order(
    obj: *const c_void,
    out: *mut u64,
    cap: u64,
) -> u64 {
    unsafe { print_order(obj, out, cap, false) }
}

/// [`__torajs_dynobj_iter_print_order`] for a face that follows
/// index keys kept elsewhere (an array's elements): strings in
/// insertion order, then the symbols, whatever the face holds.
///
/// # Safety
/// Same contract as [`__torajs_dynobj_iter_print_order`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_iter_print_order_after_index(
    obj: *const c_void,
    out: *mut u64,
    cap: u64,
) -> u64 {
    unsafe { print_order(obj, out, cap, true) }
}

unsafe fn print_order(obj: *const c_void, out: *mut u64, cap: u64, after_index: bool) -> u64 {
    if obj.is_null() || out.is_null() {
        return 0;
    }
    if unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return 0;
    }
    let len = unsafe { entries_len(obj) } as u64;
    if cap < len {
        return 0;
    }
    let mut has_index = after_index;
    for i in 0..len {
        if has_index {
            break;
        }
        let kp_tagged = unsafe { (*entries(obj).add(i as usize)).key_ptr_tagged };
        if kp_tagged == DYNOBJ_KEY_HOLE {
            continue;
        }
        has_index = unsafe { key_array_index(bucket_key_ptr(kp_tagged)) }.is_some();
    }
    let mut n = 0usize;
    if !has_index {
        // Pure insertion order, symbols in place.
        for i in 0..len {
            let kp_tagged = unsafe { (*entries(obj).add(i as usize)).key_ptr_tagged };
            if kp_tagged == DYNOBJ_KEY_HOLE {
                continue;
            }
            unsafe { *out.add(n) = i };
            n += 1;
        }
        return n as u64;
    }
    // Index keys ascending, then strings in insertion order (the
    // string-key order), then the symbols in insertion order.
    n = unsafe { __torajs_dynobj_iter_order(obj, out, cap) } as usize;
    for i in 0..len {
        let kp_tagged = unsafe { (*entries(obj).add(i as usize)).key_ptr_tagged };
        if kp_tagged == DYNOBJ_KEY_HOLE {
            continue;
        }
        if unsafe { key_is_symbol(bucket_key_ptr(kp_tagged)) } {
            unsafe { *out.add(n) = i };
            n += 1;
        }
    }
    n as u64
}

/// The number of own array-index keys — how many leading entries of
/// [`__torajs_dynobj_iter_print_order`] are index keys, so a struct
/// printer can put them before its layout fields the way bun puts
/// the butterfly before the structure's properties.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_iter_index_count(obj: *const c_void) -> u64 {
    if obj.is_null() || unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return 0;
    }
    let len = unsafe { entries_len(obj) } as u64;
    let mut n = 0u64;
    for i in 0..len {
        let kp_tagged = unsafe { (*entries(obj).add(i as usize)).key_ptr_tagged };
        if kp_tagged == DYNOBJ_KEY_HOLE {
            continue;
        }
        if unsafe { key_array_index(bucket_key_ptr(kp_tagged)) }.is_some() {
            n += 1;
        }
    }
    n
}
