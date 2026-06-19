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

use crate::layout::{ARR_LEN_OFF, ARR_SLOTS_OFF};

const ARR_HEAD_OFF: usize = 20;
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

unsafe extern "C" {
    #[link_name = "__torajs_str_alloc_pooled"]
    fn str_alloc_pooled(len: u64) -> *mut u8;
    #[link_name = "__torajs_str_drop"]
    fn str_drop(s: *mut c_void);
    fn __torajs_num_to_locale_i(n: i64) -> *mut u8;
    fn __torajs_num_to_locale_f(n: f64) -> *mut u8;
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
    unsafe {
        arr.add(ARR_SLOTS_OFF)
            .wrapping_add(((arr_head_offset(arr) + i) * 8) as usize)
    }
}

#[inline]
unsafe fn str_len(s: *const u8) -> u64 {
    unsafe { (s.add(STR_LEN_OFF) as *const u64).read() }
}

#[inline]
unsafe fn str_data(s: *const u8) -> *const u8 {
    unsafe { s.add(STR_DATA_OFF) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_i64_locale(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let sep_len = str_len(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return str_alloc_pooled(0);
        }
        let mut elem_strs: Vec<*mut u8> = Vec::with_capacity(len as usize);
        let mut total: u64 = 0;
        for i in 0..len {
            let e = *(slot_addr(arr, i) as *const i64);
            let s = __torajs_num_to_locale_i(e);
            total += str_len(s);
            elem_strs.push(s);
        }
        total += sep_len * (len - 1);
        let p = str_alloc_pooled(total);
        let p_data = p.add(STR_DATA_OFF);
        let mut cursor: u64 = 0;
        for i in 0..len {
            if i > 0 && sep_len > 0 {
                core::ptr::copy_nonoverlapping(
                    sep_data,
                    p_data.add(cursor as usize),
                    sep_len as usize,
                );
                cursor += sep_len;
            }
            let s = elem_strs[i as usize];
            let s_len = str_len(s);
            if s_len > 0 {
                core::ptr::copy_nonoverlapping(
                    str_data(s),
                    p_data.add(cursor as usize),
                    s_len as usize,
                );
                cursor += s_len;
            }
            str_drop(s as *mut c_void);
        }
        p
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_f64_locale(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let sep_len = str_len(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return str_alloc_pooled(0);
        }
        let mut elem_strs: Vec<*mut u8> = Vec::with_capacity(len as usize);
        let mut total: u64 = 0;
        for i in 0..len {
            let e = *(slot_addr(arr, i) as *const f64);
            let s = __torajs_num_to_locale_f(e);
            total += str_len(s);
            elem_strs.push(s);
        }
        total += sep_len * (len - 1);
        let p = str_alloc_pooled(total);
        let p_data = p.add(STR_DATA_OFF);
        let mut cursor: u64 = 0;
        for i in 0..len {
            if i > 0 && sep_len > 0 {
                core::ptr::copy_nonoverlapping(
                    sep_data,
                    p_data.add(cursor as usize),
                    sep_len as usize,
                );
                cursor += sep_len;
            }
            let s = elem_strs[i as usize];
            let s_len = str_len(s);
            if s_len > 0 {
                core::ptr::copy_nonoverlapping(
                    str_data(s),
                    p_data.add(cursor as usize),
                    s_len as usize,
                );
                cursor += s_len;
            }
            str_drop(s as *mut c_void);
        }
        p
    }
}
