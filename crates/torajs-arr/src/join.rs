//! `arr.join(sep)` family + `toReversed` + `with`.
//!
//! Port of `runtime_str.c::__torajs_arr_join{,_i64,_f64,_bool,_substr}`
//! + `_to_reversed` + `_with` (P4.1-h, 2026-05-23).
//!
//! Each join variant is a two-pass implementation: pass 1 sums output
//! length per-element-type, pass 2 allocates the final Str + memcpys
//! every piece + the separator between adjacent pieces.
//!
//! `_to_reversed` / `_with` are ES2023 non-mutating array updates —
//! single malloc + element-wise copy from source. T-13.5 deque-aware
//! (source `head_offset` folded via the slot pointer helpers).
//!
//! Output Str allocation goes through cross-tier
//! [`crate::str_bridge::alloc_str_raw`] which wraps `__torajs_str_alloc_pooled`
//! from libtorajs_str.a (Layer-2 sibling; same extern pattern as
//! torajs-num and torajs-bigint).

use core::ffi::c_void;

use crate::layout::{ARR_LEN_OFF, ARR_SLOTS_OFF, TAG_ARR};
use crate::str_bridge::str_alloc_pooled;

const ARR_HEAD_OFF: usize = 20;
const ARR_CAP_LOW32_OFF: usize = 16;

// Str + Substr layout mirrors (Layer-2 cross-tier).
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

const SUBSTR_LEN_OFF: usize = 8;
const SUBSTR_PARENT_OFF: usize = 16;
const SUBSTR_OFFSET_OFF: usize = 24;

unsafe extern "C" {
    fn malloc(n: usize) -> *mut c_void;
    fn snprintf(buf: *mut u8, size: usize, fmt: *const u8, ...) -> i32;
    fn strtod(s: *const u8, endptr: *mut *mut u8) -> f64;
}

// ============================================================
// Helpers
// ============================================================

#[inline]
unsafe fn arr_len(arr: *const u8) -> u64 {
    unsafe { *(arr.add(ARR_LEN_OFF) as *const u64) }
}

#[inline]
unsafe fn arr_head(arr: *const u8) -> u32 {
    unsafe { *(arr.add(ARR_HEAD_OFF) as *const u32) }
}

#[inline]
unsafe fn slot_addr(arr: *const u8, i: u64) -> *const u8 {
    unsafe {
        let head = arr_head(arr) as usize;
        arr.add(ARR_SLOTS_OFF + (head + i as usize) * 8)
    }
}

#[inline]
unsafe fn str_len(s: *const u8) -> u64 {
    unsafe { *(s.add(STR_LEN_OFF) as *const u64) }
}

#[inline]
unsafe fn str_data(s: *const u8) -> *const u8 {
    unsafe { s.add(STR_DATA_OFF) }
}

/// f64 → shortest spec-correct decimal. Port of C `torajs_f64_shortest`.
/// Integer-valued + |d| < 1e21 → `%.0f` (plain integer no exponent).
/// Else loop precision 1..=17, smallest that roundtrips via strtod.
/// Falls back to `%.17g` at precision 18 (shouldn't be reached for
/// finite values). Writes to `buf`, returns byte count (excluding NUL),
/// or -1 on overflow.
unsafe fn f64_shortest(d: f64, buf: *mut u8, cap: usize) -> i32 {
    unsafe {
        let abs_d = d.abs();
        if d == d.floor() && abs_d < 1e21 {
            return snprintf(buf, cap, b"%.0f\0".as_ptr(), d);
        }
        for prec in 1..=17 {
            let written = snprintf(buf, cap, b"%.*g\0".as_ptr(), prec as i32, d);
            if written < 0 || written as usize >= cap {
                return -1;
            }
            let parsed = strtod(buf as *const u8, core::ptr::null_mut());
            if parsed == d {
                return written;
            }
        }
        snprintf(buf, cap, b"%.17g\0".as_ptr(), d)
    }
}

/// Internal alloc for `Array<T>` (matches C's `arr_alloc_`).
/// Bypasses cap-matched pool — to_reversed/with always produce a fresh
/// right-sized block.
#[inline]
unsafe fn arr_alloc_fresh(len: u64, cap: u64) -> *mut u8 {
    unsafe {
        let total = ARR_SLOTS_OFF + (cap as usize) * 8;
        let p = malloc(total) as *mut u8;
        *(p as *mut u32) = 1;
        *(p.add(4) as *mut u16) = TAG_ARR;
        *(p.add(6) as *mut u16) = 0;
        *(p.add(ARR_LEN_OFF) as *mut u64) = len;
        *(p.add(ARR_CAP_LOW32_OFF) as *mut u32) = cap as u32;
        *(p.add(ARR_HEAD_OFF) as *mut u32) = 0;
        p
    }
}

// ============================================================
// arr_join — Array<Str>
// ============================================================

/// `Array<Str>.join(sep)`. Each slot is a `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let sep_len = str_len(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return str_alloc_pooled(0);
        }
        let mut total: u64 = 0;
        for i in 0..len {
            let elem = *(slot_addr(arr, i) as *const *const u8);
            total += str_len(elem);
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
            let elem = *(slot_addr(arr, i) as *const *const u8);
            let elem_len = str_len(elem);
            if elem_len > 0 {
                core::ptr::copy_nonoverlapping(
                    str_data(elem),
                    p_data.add(cursor as usize),
                    elem_len as usize,
                );
                cursor += elem_len;
            }
        }
        p
    }
}

// ============================================================
// arr_join_i64 — Array<I64>
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_i64(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let sep_len = str_len(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return str_alloc_pooled(0);
        }
        let mut buf = [0u8; 24];
        // pass 1: total
        let mut total: u64 = 0;
        for i in 0..len {
            let e = *(slot_addr(arr, i) as *const i64);
            let n = snprintf(
                buf.as_mut_ptr(),
                24,
                b"%lld\0".as_ptr(),
                e as core::ffi::c_longlong,
            );
            total += n.max(0) as u64;
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
            let e = *(slot_addr(arr, i) as *const i64);
            let n = snprintf(
                buf.as_mut_ptr(),
                24,
                b"%lld\0".as_ptr(),
                e as core::ffi::c_longlong,
            );
            let n = n.max(0) as usize;
            core::ptr::copy_nonoverlapping(buf.as_ptr(), p_data.add(cursor as usize), n);
            cursor += n as u64;
        }
        p
    }
}

// ============================================================
// arr_join_f64 — Array<F64>
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_f64(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let sep_len = str_len(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return str_alloc_pooled(0);
        }
        let mut buf = [0u8; 32];
        // pass 1: total
        let mut total: u64 = 0;
        for i in 0..len {
            let e = *(slot_addr(arr, i) as *const f64);
            total += if e.is_nan() {
                3 // "NaN"
            } else if e == f64::INFINITY {
                8 // "Infinity"
            } else if e == f64::NEG_INFINITY {
                9 // "-Infinity"
            } else {
                let n = f64_shortest(e, buf.as_mut_ptr(), 32);
                n.max(0) as u64
            };
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
            let e = *(slot_addr(arr, i) as *const f64);
            if e.is_nan() {
                core::ptr::copy_nonoverlapping(b"NaN".as_ptr(), p_data.add(cursor as usize), 3);
                cursor += 3;
            } else if e == f64::INFINITY {
                core::ptr::copy_nonoverlapping(
                    b"Infinity".as_ptr(),
                    p_data.add(cursor as usize),
                    8,
                );
                cursor += 8;
            } else if e == f64::NEG_INFINITY {
                core::ptr::copy_nonoverlapping(
                    b"-Infinity".as_ptr(),
                    p_data.add(cursor as usize),
                    9,
                );
                cursor += 9;
            } else {
                let n = f64_shortest(e, buf.as_mut_ptr(), 32);
                let n = n.max(0) as usize;
                core::ptr::copy_nonoverlapping(buf.as_ptr(), p_data.add(cursor as usize), n);
                cursor += n as u64;
            }
        }
        p
    }
}

// ============================================================
// arr_join_bool — Array<Bool>
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_bool(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let sep_len = str_len(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return str_alloc_pooled(0);
        }
        let mut total: u64 = 0;
        for i in 0..len {
            let e = *(slot_addr(arr, i) as *const i64);
            total += if e != 0 { 4 } else { 5 };
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
            let e = *(slot_addr(arr, i) as *const i64);
            if e != 0 {
                core::ptr::copy_nonoverlapping(b"true".as_ptr(), p_data.add(cursor as usize), 4);
                cursor += 4;
            } else {
                core::ptr::copy_nonoverlapping(b"false".as_ptr(), p_data.add(cursor as usize), 5);
                cursor += 5;
            }
        }
        p
    }
}

// ============================================================
// arr_join_substr — Array<Substr>
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_substr(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let sep_len = str_len(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return str_alloc_pooled(0);
        }
        // pass 1: total
        let mut total: u64 = 0;
        for i in 0..len {
            let v = *(slot_addr(arr, i) as *const *const u8);
            total += *(v.add(SUBSTR_LEN_OFF) as *const u64);
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
            let v = *(slot_addr(arr, i) as *const *const u8);
            let v_len = *(v.add(SUBSTR_LEN_OFF) as *const u64);
            if v_len > 0 {
                let parent = *(v.add(SUBSTR_PARENT_OFF) as *const *const u8);
                let v_off = *(v.add(SUBSTR_OFFSET_OFF) as *const u64);
                core::ptr::copy_nonoverlapping(
                    parent.add(STR_DATA_OFF + v_off as usize),
                    p_data.add(cursor as usize),
                    v_len as usize,
                );
                cursor += v_len;
            }
        }
        p
    }
}

// ============================================================
// arr_to_reversed — ES2023 non-mutating reverse
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_to_reversed(arr: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let p = arr_alloc_fresh(len, len);
        let dst = p.add(ARR_SLOTS_OFF);
        for i in 0..len {
            let src = slot_addr(arr, len - 1 - i);
            *(dst.add(i as usize * 8) as *mut u64) = *(src as *const u64);
        }
        p
    }
}

// ============================================================
// arr_with — ES2023 non-mutating index update
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_with(arr: *const u8, i: i64, v: i64) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let p = arr_alloc_fresh(len, len);
        let dst = p.add(ARR_SLOTS_OFF);
        if len > 0 {
            let src = slot_addr(arr, 0);
            core::ptr::copy_nonoverlapping(src, dst, (len as usize) * 8);
        }
        let adj = if i < 0 { len as i64 + i } else { i };
        *(dst.add(adj as usize * 8) as *mut u64) = v as u64;
        p
    }
}
