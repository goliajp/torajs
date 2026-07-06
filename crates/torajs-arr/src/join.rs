//! `arr.join(sep)` family + ES2023 `toReversed` + `with`. Port of
//! `runtime_str.c::__torajs_arr_join{,_i64,_f64,_bool,_substr}` +
//! `_to_reversed` + `_with` (P4.1-h, 2026-05-23). Each join is
//! two-pass (sum lengths, then alloc + memcpy + interleaved sep);
//! the non-mutating updates are single malloc + deque-aware element
//! copy. Output Str allocation goes through cross-tier
//! [`crate::str_bridge::alloc_str_raw`] (wraps
//! `__torajs_str_alloc_pooled` from libtorajs_str.a).

use core::ffi::c_void;

use crate::layout::{ARR_CELL_SIZE, ARR_DATA_PTR_OFF, ARR_LEN_OFF, TAG_ARR, arr_data};
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
    /// torajs-mmalloc libc-compat malloc — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    /// libc-compat free — pair with `malloc` for transient buffers.
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
    // v0.7-A4 Step 15-d: 0-libc f64 → shortest decimal + i64 →
    // decimal via torajs-fmt. Replaces libc snprintf %.*g loop +
    // snprintf "%lld" for the i64-join path.
    fn __torajs_fmt_dtoa(d: f64, out_buf: *mut u8, out_cap: usize) -> i32;
    fn __torajs_fmt_itoa(n: i64, out_buf: *mut u8, out_cap: usize) -> i32;
    /// `ToString(v)` per ES §7.1.17. Returns a freshly-owned Str ptr
    /// the caller must drop. Defined in `torajs-anyvalue::nanbox_ffi`.
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    /// Drop a Str (rc_dec; free if rc reaches 0). Defined in
    /// `torajs-str::drop` (Layer-2 sibling).
    #[link_name = "__torajs_str_drop"]
    fn str_drop(s: *mut c_void);
    /// Cross-tier — torajs-throw. Records a pending RangeError via TLS;
    /// returns normally so the caller's emit_throw_check after the
    /// call site propagates the throw (non-local via TLS, not stack
    /// unwind).
    fn __torajs_throw_range_error(msg: *const u8);
}

// AnyValue NaN-box constants — match `torajs-anyvalue::nanbox`
// (VALUE_NULL = TAG_BIT_TYPE_OTHER, VALUE_UNDEFINED =
// TAG_BIT_TYPE_OTHER | TAG_BIT_UNDEFINED). Spec §22.1.3.15.5:
// undefined / null → empty String. Detect at the tag level to skip
// the alloc+drop round-trip — `__torajs_anyv_to_str` follows ES
// §7.1.17 ToString and returns "undefined" / "null" literally.
const VALUE_NULL_IMM: u64 = 0x0000_0000_0000_0002;
const VALUE_UNDEFINED_IMM: u64 = 0x0000_0000_0000_000A;

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
        arr_data(arr).add((head + i as usize) * 8)
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

/// f64 → shortest spec-correct decimal. v0.7-A4 Step 15-d:
/// delegates to torajs-fmt's `__torajs_fmt_dtoa` (0-libc;
/// core::fmt Grisu3 + JS-spec post-process). Same shortest-
/// roundtrip + ES §6.1.6.1.13 shape as the prior libc-based
/// implementation, but in a single call instead of try-
/// precisions loop.
unsafe fn f64_shortest(d: f64, buf: *mut u8, cap: usize) -> i32 {
    unsafe { __torajs_fmt_dtoa(d, buf, cap) }
}

/// Internal alloc for `Array<T>` (matches C's `arr_alloc_`).
/// Bypasses cap-matched pool — to_reversed/with always produce a fresh
/// right-sized block.
#[inline]
unsafe fn arr_alloc_fresh(len: u64, cap: u64) -> *mut u8 {
    unsafe {
        let total = ARR_CELL_SIZE + (cap as usize) * 8;
        let p = malloc(total) as *mut u8;
        *(p as *mut u32) = 1;
        *(p.add(4) as *mut u16) = TAG_ARR;
        *(p.add(6) as *mut u16) = 0;
        *(p.add(ARR_LEN_OFF) as *mut u64) = len;
        *(p.add(ARR_CAP_LOW32_OFF) as *mut u32) = cap as u32;
        *(p.add(ARR_HEAD_OFF) as *mut u32) = 0;
        // props NULL (chunk 516 slice fix, same latent-garbage class)
        // + B1 self-referential data pointer.
        *(p.add(crate::layout::ARR_PROPS_OFF) as *mut u64) = 0;
        *(p.add(ARR_DATA_PTR_OFF) as *mut *mut u8) = p.add(ARR_CELL_SIZE);
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
            let n = __torajs_fmt_itoa(e, buf.as_mut_ptr(), 24);
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
            let n = __torajs_fmt_itoa(e, buf.as_mut_ptr(), 24);
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
// arr_join_any — Array<Any>
// ============================================================

/// `Array<Any>.join(sep)`. Each slot is a NaN-box `AnyValue` u64
/// (Step 7e-A — 8-byte stride matches the typed-tier helpers, so
/// `slot_addr` reuses without a custom stride helper).
///
/// Spec §22.1.3.15.5: per-element ToString is delegated to
/// `__torajs_anyv_to_str` (`torajs-anyvalue::nanbox_ffi`), which
/// honors ES §7.1.17 — but Array.join overrides ToString for
/// undefined / null to the empty string, so this layer special-
/// cases the two sentinels before the helper call to skip the
/// "undefined" / "null" literal that `anyv_to_str` would produce
/// and to elide the alloc+drop round-trip.
///
/// Per-element Str alloc must be dropped after copy — the joined
/// result owns the final bytes; the temporaries are transient.
/// Holds the temp ptrs in a heap Vec (rather than a second pass
/// recomputing each ToString) so each slot's ToString runs once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_any(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let sep_len = str_len(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return str_alloc_pooled(0);
        }
        // pass 1: ToString each slot, cache the resulting Str ptrs
        // (NULL for undefined/null which contribute the empty string).
        let tmp_bytes = (len as usize) * core::mem::size_of::<*mut c_void>();
        let tmp = malloc(tmp_bytes) as *mut *mut c_void;
        let mut total: u64 = 0;
        for i in 0..len {
            let av = *(slot_addr(arr, i) as *const u64);
            if av == VALUE_NULL_IMM || av == VALUE_UNDEFINED_IMM {
                *tmp.add(i as usize) = core::ptr::null_mut();
            } else {
                let s = __torajs_anyv_to_str(av);
                *tmp.add(i as usize) = s;
                total += str_len(s as *const u8);
            }
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
            let s = *tmp.add(i as usize);
            if !s.is_null() {
                let s_u8 = s as *const u8;
                let elem_len = str_len(s_u8);
                if elem_len > 0 {
                    core::ptr::copy_nonoverlapping(
                        str_data(s_u8),
                        p_data.add(cursor as usize),
                        elem_len as usize,
                    );
                    cursor += elem_len;
                }
                str_drop(s);
            }
        }
        free(tmp as *mut c_void);
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
        let dst = arr_data(p);
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
        // ES §23.1.3.39 step 7 — `actualIndex = i < 0 ? len + i : i`,
        // throw RangeError when out-of-range. Pre-fix this fell
        // through to the unchecked slot store and silently corrupted
        // adjacent heap (or wrote past the alloc on small caps).
        let adj = if i < 0 { len as i64 + i } else { i };
        if adj < 0 || adj >= len as i64 {
            __torajs_throw_range_error(b"Array index out of range\0".as_ptr());
            return core::ptr::null_mut();
        }
        let p = arr_alloc_fresh(len, len);
        let dst = arr_data(p);
        if len > 0 {
            let src = slot_addr(arr, 0);
            core::ptr::copy_nonoverlapping(src, dst, (len as usize) * 8);
        }
        *(dst.add(adj as usize * 8) as *mut u64) = v as u64;
        p
    }
}
