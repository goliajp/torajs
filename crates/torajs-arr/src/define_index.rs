//! Canonical-index arm of the Array DefineOwnProperty kernel —
//! split from `define.rs` (file-size hard limit; rotation 267 刀 R5a
//! added the `throw_on_refusal` parameterization + soft shell and
//! pushed the shared file over 500). Dispatch stays in
//! [`crate::define::__torajs_arr_define`]'s impl.

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_LENGTH_RO, FLAG_NON_EXTENSIBLE, HeapHeader, Tag};

use crate::define::{
    ANY_HEAP, ANY_UNDEF, F_CONFIGURABLE, F_ENUMERABLE, F_HOLE, F_WRITABLE, FLAGS_DEFAULT,
    P_CONFIGURABLE, P_ENUMERABLE, P_VALUE, P_WRITABLE, header_flags, props_slot, refuse,
    store_shadow,
};
use crate::define_index_flags::index_flags_with_key;
use crate::layout::ARR_LEN_OFF;

unsafe extern "C" {
    /// torajs-dynobj — entry removal (drops key + value; the
    /// accessor→data transition deletes the pair-owning shadow entry).
    fn __torajs_dynobj_delete(dynobj: *mut c_void, key: *const c_void) -> i32;
    /// torajs-str — view-aware content equality (SameValue on Str).
    fn __torajs_str_eq(a: *const u8, b: *const u8) -> i64;
}

/// §7.2.10 SameValue on unboxed `(tag, value)` pairs. Bit equality
/// covers immediates (F64 bits distinguish ±0 and unify NaN) and
/// heap identity; equal-content Str cells at different addresses
/// compare by content (view-aware `str_eq`), and a mixed-width
/// number pair (I64 5 vs F64 5.0) is the same Number value.
unsafe fn same_value_pair(a_tag: u64, a_val: u64, b_tag: u64, b_val: u64) -> bool {
    if a_tag == b_tag && a_val == b_val {
        return true;
    }
    const ANY_I64: u64 = 2;
    const ANY_F64: u64 = 3;
    if a_tag == ANY_I64 && b_tag == ANY_F64 {
        // bits-equal keeps ±0 apart (i as f64 is always +0); the
        // round-trip check rejects a lossy i64 → f64 conversion.
        let (i, d) = (a_val as i64, f64::from_bits(b_val));
        return d.to_bits() == (i as f64).to_bits() && d as i64 == i;
    }
    if a_tag == ANY_F64 && b_tag == ANY_I64 {
        return unsafe { same_value_pair(b_tag, b_val, a_tag, a_val) };
    }
    if a_tag != ANY_HEAP || b_tag != ANY_HEAP || a_val == 0 || b_val == 0 {
        return false;
    }
    let a_tt = unsafe { (*(a_val as *const HeapHeader)).type_tag };
    let b_tt = unsafe { (*(b_val as *const HeapHeader)).type_tag };
    if a_tt != Tag::Str as u16 || b_tt != Tag::Str as u16 {
        return false;
    }
    unsafe { __torajs_str_eq(a_val as *const u8, b_val as *const u8) != 0 }
}

/// Validate + apply for a canonical index per §10.1.6.3 (data
/// subset; the readonly value check runs §7.2.10 SameValue via
/// [`same_value_pair`]). Answers 1 on success, 0 on a §10.1.6.3
/// refusal — which records the TypeError only for the throwing
/// flavor (see [`refuse`]).
pub(crate) unsafe fn define_index(
    arr: *mut c_void,
    key: *mut c_void,
    idx: u64,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    let len = unsafe { (arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    let has_value = flags_byte & P_VALUE != 0;

    if idx < len {
        let cur_flags = unsafe { index_flags_with_key(arr, key as *const c_void) };
        // A hole is an absent property — the define is a fresh
        // CreateDataProperty (extensible check, no current-flags
        // validation) that revives the index (chunk C).
        if cur_flags & F_HOLE != 0 {
            if unsafe { header_flags(arr) } & FLAG_NON_EXTENSIBLE != 0 {
                return unsafe {
                    refuse(
                        throw_on_refusal,
                        c"Attempting to define property on object that is not extensible.".as_ptr(),
                        tag,
                        value,
                    )
                };
            }
            if has_value {
                unsafe { crate::index_any::__torajs_arr_index_set(arr, idx as i64, tag, value) };
            }
            // Unconditional shadow write — the flags upsert also
            // clears the hole sentinel, so a defaults-flags define
            // must still land.
            unsafe { store_shadow(arr, key, flags_byte & FLAGS_DEFAULT) };
            return 1;
        }
        // Accessor → data transition (chunk C): a data descriptor over
        // an accessor index needs the configurable gate, then the
        // shadow entry (owning the pair) is deleted and the define
        // lands as a data property — absent `writable` completes to
        // false, e/c keep the current values (§10.1.6.3 step 4).
        let pair = unsafe { crate::define_accessor::__torajs_arr_index_accessor(arr, idx) };
        if !pair.is_null() {
            let cur_e = cur_flags & F_ENUMERABLE != 0;
            let cur_c = cur_flags & F_CONFIGURABLE != 0;
            if !cur_c {
                return unsafe {
                    refuse(
                        throw_on_refusal,
                        c"Attempting to change configurable attribute of unconfigurable property."
                            .as_ptr(),
                        tag,
                        value,
                    )
                };
            }
            let props = unsafe { *props_slot(arr) };
            unsafe { __torajs_dynobj_delete(props, key as *const c_void) };
            if has_value {
                unsafe { crate::index_any::__torajs_arr_index_set(arr, idx as i64, tag, value) };
            }
            let mut new_flags = 0u64;
            if flags_byte & P_WRITABLE != 0 && flags_byte & F_WRITABLE != 0 {
                new_flags |= F_WRITABLE;
            }
            let e = if flags_byte & P_ENUMERABLE != 0 {
                flags_byte & F_ENUMERABLE != 0
            } else {
                cur_e
            };
            let c = if flags_byte & P_CONFIGURABLE != 0 {
                flags_byte & F_CONFIGURABLE != 0
            } else {
                cur_c
            };
            if e {
                new_flags |= F_ENUMERABLE;
            }
            if c {
                new_flags |= F_CONFIGURABLE;
            }
            unsafe { store_shadow(arr, key, new_flags) };
            return 1;
        }
        let cur_w = cur_flags & F_WRITABLE != 0;
        let cur_e = cur_flags & F_ENUMERABLE != 0;
        let cur_c = cur_flags & F_CONFIGURABLE != 0;
        if !cur_c {
            if flags_byte & P_CONFIGURABLE != 0 && flags_byte & F_CONFIGURABLE != 0 {
                return unsafe {
                    refuse(
                        throw_on_refusal,
                        c"Attempting to change configurable attribute of unconfigurable property."
                            .as_ptr(),
                        tag,
                        value,
                    )
                };
            }
            if flags_byte & P_ENUMERABLE != 0 && (flags_byte & F_ENUMERABLE != 0) != cur_e {
                return unsafe {
                    refuse(
                        throw_on_refusal,
                        c"Attempting to change enumerable attribute of unconfigurable property."
                            .as_ptr(),
                        tag,
                        value,
                    )
                };
            }
            if !cur_w {
                if flags_byte & P_WRITABLE != 0 && flags_byte & F_WRITABLE != 0 {
                    return unsafe {
                        refuse(
                            throw_on_refusal,
                            c"Attempting to change writable attribute of unconfigurable property."
                                .as_ptr(),
                            tag,
                            value,
                        )
                    };
                }
                if has_value {
                    let cur_tag = unsafe { crate::any::__torajs_arr_get_any_tag(arr, idx) };
                    let cur_val = unsafe { crate::any::__torajs_arr_get_any_value(arr, idx) };
                    if !unsafe { same_value_pair(tag, value, cur_tag, cur_val) } {
                        return unsafe {
                            refuse(
                                throw_on_refusal,
                                c"Attempting to change value of a readonly property.".as_ptr(),
                                tag,
                                value,
                            )
                        };
                    }
                }
            }
        }
        if has_value {
            // index_set consumes the transferred rc (drop-old +
            // store-new, kind-aware).
            unsafe { crate::index_any::__torajs_arr_index_set(arr, idx as i64, tag, value) };
        }
        let new_flags = fold_flags(cur_flags, flags_byte);
        if new_flags != cur_flags {
            unsafe { store_shadow(arr, key, new_flags) };
        }
        return 1;
    }

    // Fresh index — §10.4.2.1 step 2: a locked length (chunk D)
    // rejects the implicit length bump before the extensible check.
    if unsafe { header_flags(arr) } & FLAG_ARR_LENGTH_RO != 0 {
        return unsafe {
            refuse(
                throw_on_refusal,
                c"Attempting to define property beyond a non-writable array length.".as_ptr(),
                tag,
                value,
            )
        };
    }
    if unsafe { header_flags(arr) } & FLAG_NON_EXTENSIBLE != 0 {
        return unsafe {
            refuse(
                throw_on_refusal,
                c"Attempting to define property on object that is not extensible.".as_ptr(),
                tag,
                value,
            )
        };
    }
    // Dense model: fill the gap with undefined elements, then append
    // the defined value — and mark the fill a hole range, because
    // §10.4.2.1 raises `length` to cover the defined index and creates
    // nothing in between. (This was a recorded divergence: the fill
    // positions read as own properties. It stopped being affordable
    // when a hole gate started consulting the same answer — an
    // `Array.prototype[5] = v` made indices 0..4 own on every array
    // that inherits from it.)
    let mut cursor = len;
    while cursor < idx {
        unsafe { crate::any::__torajs_arr_push_any(arr, ANY_UNDEF, 0) };
        cursor += 1;
    }
    if idx > len {
        unsafe { crate::define_hole::mark_hole_range(arr, len, idx) };
    }
    let (init_tag, init_value) = if has_value {
        (tag, value)
    } else {
        (ANY_UNDEF, 0)
    };
    unsafe { crate::any::__torajs_arr_push_any(arr, init_tag, init_value) };
    // Fresh define completes absent flags to false (§10.1.6.2).
    let new_flags = flags_byte & FLAGS_DEFAULT;
    if new_flags != FLAGS_DEFAULT {
        unsafe { store_shadow(arr, key, new_flags) };
    }
    1
}

/// Per-flag fold: present → descriptor value, absent → current.
fn fold_flags(cur: u64, flags_byte: u64) -> u64 {
    let pick = |present: u64, val: u64, cur_bit: u64| -> u64 {
        if flags_byte & present != 0 {
            if flags_byte & val != 0 { val } else { 0 }
        } else {
            cur & cur_bit
        }
    };
    pick(P_WRITABLE, F_WRITABLE, F_WRITABLE)
        | pick(P_ENUMERABLE, F_ENUMERABLE, F_ENUMERABLE)
        | pick(P_CONFIGURABLE, F_CONFIGURABLE, F_CONFIGURABLE)
}
