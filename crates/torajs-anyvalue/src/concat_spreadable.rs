//! `Array.prototype.concat`'s per-item step (§23.1.3.1 step 5) for the
//! items the raw-slot kernels cannot answer.
//!
//! The torajs-arr concat kernels spread a plain `Tag::Arr` by copying
//! its slots and append everything else as one element. That is the
//! whole answer only when nothing observable sits between: no
//! `@@isConcatSpreadable` on the item or its chain (§7.3.22 step 2 —
//! the getter runs, and a truthy answer spreads a non-array, a falsy
//! one appends an array), no hole to keep (step 5.b.iv.2 — an absent
//! index advances `n` without creating a property), no accessor index
//! to run, no Proxy whose IsArray is its target's. [`needs_spec`]
//! answers whether an item is one of those; [`spread_spec`] runs the
//! spec step for it, writing into the kernel's accumulator.
//!
//! Holes in the product are made by length, not by a store: an absent
//! index leaves `n` ahead of the accumulator's length, and the next
//! present element (or the item's end) grows the length to `n` first,
//! which marks the gap as holes (`__torajs_arr_set_length_any`).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get_layout::recv_cell;
use crate::method_call_arraylike::{arraylike_get, arraylike_has};
use crate::nanbox::{AnyValue, is_undefined};

unsafe extern "C" {
    fn __torajs_arr_push_any_boxed(arr: *mut u8, av: u64) -> *mut u8;
    fn __torajs_arr_set_length_any(arr: *mut c_void, tag: i64, value: i64);
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
}

/// §6.1.5.1 well-known symbol index of `@@isConcatSpreadable`
/// (torajs-str's alphabetical `WELL_KNOWN_SLOTS`).
const WK_IS_CONCAT_SPREADABLE: i64 = 4;

/// 2^53 - 1 — §23.1.3.1 steps 5.b.iii / 5.c.i.
const MAX_SAFE_LEN: i64 = 9_007_199_254_740_991;

const ARR_LEN_OFF: usize = 8;

/// Primitive cells: not Objects, so §7.3.22 step 1 answers false and
/// the fast append is the spec answer.
fn is_primitive_cell(tag: u16) -> bool {
    tag == Tag::Str as u16 || tag == Tag::Symbol as u16 || tag == Tag::BigInt as u16
}

/// Does `av` need [`spread_spec`]? 0 = the kernel's own step is exact.
///
/// An Array needs it only with a hole shadow / accessor index or an
/// `@@isConcatSpreadable` somewhere on its chain; any other object
/// needs it always (its own entry decides, and without one it is not
/// spreadable — which the spec step also answers).
///
/// # Safety
/// `av` is a borrowed AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_concat_needs_spec(av: AnyValue) -> i64 {
    let Some((ptr, tag)) = recv_cell(av) else {
        return 0;
    };
    if is_primitive_cell(tag) {
        return 0;
    }
    if tag != Tag::Arr as u16 {
        return 1;
    }
    unsafe {
        let flags = (ptr.cast::<u8>().add(6) as *const u16).read();
        if flags & torajs_rc::FLAG_ARR_EXOTIC_INDEX != 0 {
            return 1;
        }
        let sym = __torajs_symbol_well_known(WK_IS_CONCAT_SPREADABLE);
        let has = crate::member_get_symbol::symbol_key_has(av, sym);
        __torajs_value_drop_heap(sym);
        has as i64
    }
}

/// §7.2.2 IsArray — a Proxy answers its target's, a revoked one throws.
/// `None` = pending throw.
unsafe fn is_array(av: AnyValue) -> Option<bool> {
    let (ptr, tag) = recv_cell(av)?;
    if tag == Tag::Arr as u16 {
        return Some(true);
    }
    if tag != Tag::Proxy as u16 {
        return Some(false);
    }
    let slots = unsafe { crate::proxy::live_slots(ptr) }.ok()?;
    unsafe { is_array(slots.target) }
}

/// §7.3.22 IsConcatSpreadable. `None` = pending throw.
unsafe fn is_concat_spreadable(av: AnyValue) -> Option<bool> {
    // Step 1 — not an Object.
    match recv_cell(av) {
        Some((_, t)) if !is_primitive_cell(t) => {}
        _ => return Some(false),
    }
    unsafe {
        let sym = __torajs_symbol_well_known(WK_IS_CONCAT_SPREADABLE);
        let got = crate::member_get_symbol::symbol_key_get(av, sym);
        __torajs_value_drop_heap(sym);
        if __torajs_throw_check() != 0 {
            return None;
        }
        let (t, v) = got.pair();
        let boxed = crate::nanbox_encode::__torajs_anyv_box_from_pair(t as i64, v as i64);
        if !is_undefined(boxed) {
            return Some(crate::nanbox_ffi::__torajs_anyv_to_bool(boxed));
        }
        is_array(av)
    }
}

/// Grow `dst`'s length to `n` (the gap becomes holes).
unsafe fn pad_to(dst: *mut u8, n: i64) {
    unsafe {
        let len = (dst.add(ARR_LEN_OFF) as *const u64).read() as i64;
        if len < n {
            __torajs_arr_set_length_any(dst.cast(), 2, n);
        }
    }
}

/// §23.1.3.1 step 5 for one item `e`, appending into `dst` (an
/// `Array<Any>` accumulator whose length is `n`). Returns the
/// accumulator (it may move); on a pending throw it stops where the
/// throw happened and the caller's throw check propagates.
///
/// # Safety
/// `dst` is a live `FLAG_ARR_ANY` Array cell; `e` is borrowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_concat_spread_spec(mut dst: *mut u8, e: AnyValue) -> *mut u8 {
    unsafe {
        let n = (dst.add(ARR_LEN_OFF) as *const u64).read() as i64;
        let Some(spreadable) = is_concat_spreadable(e) else {
            return dst;
        };
        if !spreadable {
            if n >= MAX_SAFE_LEN {
                __torajs_throw_type_error(
                    c"concat cannot produce an array of length larger than (2 ** 53) - 1".as_ptr(),
                );
                return dst;
            }
            return __torajs_arr_push_any_boxed(dst, e);
        }
        let (ptr, tag) = recv_cell(e).expect("step 1 answered false for a non-object");
        let len = if tag == Tag::Arr as u16 {
            (ptr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() as i64
        } else {
            match crate::method_call_arraylike_host::arraylike_len(ptr) {
                Some(l) => l,
                None => return dst,
            }
        };
        if n + len > MAX_SAFE_LEN {
            __torajs_throw_type_error(
                c"concat cannot produce an array of length larger than (2 ** 53) - 1".as_ptr(),
            );
            return dst;
        }
        let mut k = 0;
        while k < len {
            if arraylike_has(ptr, k) {
                if __torajs_throw_check() != 0 {
                    return dst;
                }
                let v = arraylike_get(ptr, k);
                if __torajs_throw_check() != 0 {
                    return dst;
                }
                pad_to(dst, n + k);
                // The push takes its own stake; `v` is ours.
                dst = __torajs_arr_push_any_boxed(dst, v);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
            } else if __torajs_throw_check() != 0 {
                return dst;
            }
            k += 1;
        }
        pad_to(dst, n + len);
        dst
    }
}
