//! `arr.toLocaleString()` for numeric `Array<T>` per ES §22.1.3.32.
//!
//! Sibling to [`crate::join`] — same two-pass + memcpy skeleton, but
//! the per-element ToString step is replaced with ToLocaleString:
//! integer (`I64`) and floating (`F64`) slots route through
//! `__torajs_num_to_locale_i` / `_f` from `torajs-num`, which yield
//! the en-US group-separated form (`1234567` → `"1,234,567"`).
//!
//! `String` / `Substr` / `Bool` `Array<T>` are NOT covered here:
//! their element-`toLocaleString` is observably identical to
//! `toString`, so `ssa_lower_str.rs` keeps routing those receivers
//! through the existing `arr_join_substr` / `arr_join` /
//! `arr_join_bool` helpers.
//!
//! The intermediate `Vec<*mut u8>` between the two passes avoids
//! re-running `num_to_locale_*` (each call allocates + formats);
//! `toLocaleString` is a display path, not a hot loop.

use core::ffi::c_void;

use crate::join_enc::{
    STR_DATA_OFF, alloc_join_out, emit_units, str_data, str_is_latin1, str_units,
};
use crate::layout::{ARR_LEN_OFF, arr_data};

const ARR_HEAD_OFF: usize = 20;

unsafe extern "C" {
    #[link_name = "__torajs_str_drop"]
    fn str_drop(s: *mut c_void);
    fn __torajs_num_to_locale_i(n: i64) -> *mut u8;
    fn __torajs_num_to_locale_f(n: f64) -> *mut u8;
}

/// RFC 20260721 刀 5 G3 — exotic-index receivers (accessor / hole /
/// length-grow indices) leave the raw fast lanes for the per-element
/// Invoke walk, which reads kind-aware (getters run, holes consult
/// the prototype digit keys). The walk sits behind the
/// `__torajs_arr_join_exotic` link seam (`crate::exotic_seam`).
#[inline]
unsafe fn is_exotic(arr: *const u8) -> bool {
    unsafe {
        (*(arr as *const torajs_rc::HeapHeader)).flags & torajs_rc::FLAG_ARR_EXOTIC_INDEX != 0
    }
}

#[inline]
unsafe fn arr_len(arr: *const u8) -> u64 {
    unsafe { (arr.add(ARR_LEN_OFF) as *const u64).read() }
}

#[inline]
unsafe fn arr_head_offset(arr: *const u8) -> u64 {
    unsafe { (arr.add(ARR_HEAD_OFF) as *const u32).read() as u64 }
}

#[inline]
unsafe fn slot_addr(arr: *const u8, i: u64) -> *const u8 {
    unsafe { arr_data(arr).wrapping_add(((arr_head_offset(arr) + i) * 8) as usize) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_i64_locale(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        if is_exotic(arr) {
            return crate::exotic_seam::__torajs_arr_join_exotic(arr, sep, 0, 1);
        }
        let len = arr_len(arr);
        let sep_units = str_units(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return alloc_join_out(0, true);
        }
        let mut elem_strs: Vec<*mut u8> = Vec::with_capacity(len as usize);
        let mut total: u64 = 0;
        let mut out_latin1 = if len > 1 && sep_units > 0 {
            str_is_latin1(sep)
        } else {
            true
        };
        for i in 0..len {
            let e = *(slot_addr(arr, i) as *const i64);
            let s = __torajs_num_to_locale_i(e);
            let units = str_units(s);
            total += units;
            if units > 0 {
                out_latin1 &= str_is_latin1(s);
            }
            elem_strs.push(s);
        }
        total += sep_units * (len - 1);
        let p = alloc_join_out(total, out_latin1);
        let p_data = p.add(STR_DATA_OFF);
        let sep_latin1 = str_is_latin1(sep);
        let mut cursor: u64 = 0;
        for i in 0..len {
            if i > 0 && sep_units > 0 {
                emit_units(p_data, out_latin1, cursor, sep_data, sep_units, sep_latin1);
                cursor += sep_units;
            }
            let s = elem_strs[i as usize];
            let units = str_units(s);
            if units > 0 {
                emit_units(
                    p_data,
                    out_latin1,
                    cursor,
                    str_data(s),
                    units,
                    str_is_latin1(s),
                );
                cursor += units;
            }
            str_drop(s as *mut c_void);
        }
        p
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_f64_locale(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        if is_exotic(arr) {
            return crate::exotic_seam::__torajs_arr_join_exotic(arr, sep, 0, 1);
        }
        let len = arr_len(arr);
        let sep_units = str_units(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return alloc_join_out(0, true);
        }
        let mut elem_strs: Vec<*mut u8> = Vec::with_capacity(len as usize);
        let mut total: u64 = 0;
        let mut out_latin1 = if len > 1 && sep_units > 0 {
            str_is_latin1(sep)
        } else {
            true
        };
        for i in 0..len {
            let e = *(slot_addr(arr, i) as *const f64);
            let s = __torajs_num_to_locale_f(e);
            let units = str_units(s);
            total += units;
            if units > 0 {
                out_latin1 &= str_is_latin1(s);
            }
            elem_strs.push(s);
        }
        total += sep_units * (len - 1);
        let p = alloc_join_out(total, out_latin1);
        let p_data = p.add(STR_DATA_OFF);
        let sep_latin1 = str_is_latin1(sep);
        let mut cursor: u64 = 0;
        for i in 0..len {
            if i > 0 && sep_units > 0 {
                emit_units(p_data, out_latin1, cursor, sep_data, sep_units, sep_latin1);
                cursor += sep_units;
            }
            let s = elem_strs[i as usize];
            let units = str_units(s);
            if units > 0 {
                emit_units(
                    p_data,
                    out_latin1,
                    cursor,
                    str_data(s),
                    units,
                    str_is_latin1(s),
                );
                cursor += units;
            }
            str_drop(s as *mut c_void);
        }
        p
    }
}
