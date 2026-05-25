//! Promise.all / allSettled / race / any sync combinators.
//!
//! Port of `runtime_promise.c` T-17.a, T-17.b, T-17.c, T-17.d
//! sections (P6.1, 2026-05-24). MVP sync-only — synchronous fast
//! path for inputs that are all already settled at call time.
//! Pending input → reject with phase-pointer placeholder. Real
//! callback-based fan-in (count down to fire result on last
//! resolve) lands post-T-15.g.6.
//!
//! Array layout reads use the raw byte-offset accessors from
//! `crate::layout` — torajs-promise carries its own knowledge of
//! the Array<T> 8B-stride layout rather than depending on
//! torajs-arr (mirrors the C source's pattern of independent
//! layout knowledge in this section).

use core::ffi::c_void;

use crate::layout::{
    ALLSETTLED_OBJ_HEADER_SIZE, ALLSETTLED_OBJ_TAG, ARR_HDR_SIZE, ARR_HEAD_OFF, ARR_LEN_OFF,
    Promise, STATE_FULFILLED, STATE_PENDING, STATE_REJECTED, STR_HDR_SIZE,
};
use crate::pool::{
    __torajs_promise_alloc_fulfilled, __torajs_promise_alloc_fulfilled_heap,
    __torajs_promise_alloc_rejected, __torajs_promise_alloc_rejected_heap,
};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;

    fn __torajs_rc_inc(p: *mut c_void);

    /// Array<i64> alloc + push, defined in libtorajs_arr.a. Used by
    /// Promise.all / allSettled to build the result Array.
    fn __torajs_arr_alloc(initial_cap: u64) -> *mut c_void;
    fn __torajs_arr_push(arr: *mut c_void, val: i64) -> *mut c_void;

    /// Str allocator (libtorajs_str.a). Returns ptr to the body
    /// past STR_HDR_SIZE; allsettled status-string copy memcpys
    /// the literal into the body region.
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;

    /// torajs-mmalloc memcpy — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_memcpy"]
    fn memcpy(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void;
}

/// Read logical Array<T> slot `i` from `arr` (8B stride; pointer-
/// shape values stored as raw bits).
#[inline]
unsafe fn arr_slot_ptr(arr: *mut c_void, i: u64) -> *mut Promise {
    unsafe {
        let bytes = arr as *mut u8;
        let head = *(bytes.add(ARR_HEAD_OFF) as *const u32) as u64;
        let data = bytes.add(ARR_HDR_SIZE);
        let slot_off = (head + i) * 8;
        *(data.add(slot_off as usize) as *mut *mut Promise)
    }
}

#[inline]
unsafe fn arr_len(arr: *mut c_void) -> u64 {
    unsafe { *((arr as *mut u8).add(ARR_LEN_OFF) as *const u64) }
}

// ============================================================
// Promise.all<T>(Promise<T>[]) → Promise<T[]>
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_all_sync(promises_arr: *mut c_void) -> *mut c_void {
    if promises_arr.is_null() {
        return unsafe { __torajs_promise_alloc_rejected(0) };
    }
    let len = unsafe { arr_len(promises_arr) };
    // Pre-scan: first rejected → reject outer with that reason; first
    // pending → reject with placeholder (MVP — no fan-in yet).
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        if pp.is_null() {
            continue;
        }
        let state = unsafe { (*pp).state };
        if state == STATE_REJECTED {
            return unsafe { __torajs_promise_alloc_rejected((*pp).value) };
        }
        if state == STATE_PENDING {
            return unsafe { __torajs_promise_alloc_rejected(0) };
        }
    }
    // All fulfilled — build result Array.
    let mut result_arr = unsafe { __torajs_arr_alloc(len) };
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        let v = if pp.is_null() {
            0
        } else {
            unsafe { (*pp).value }
        };
        result_arr = unsafe { __torajs_arr_push(result_arr, v) };
    }
    unsafe { __torajs_promise_alloc_fulfilled_heap(result_arr as i64) }
}

// ============================================================
// Promise.allSettled
// ============================================================

const STATUS_FULFILLED_LIT: &[u8] = b"fulfilled";
const STATUS_REJECTED_LIT: &[u8] = b"rejected";

unsafe fn make_settled_str(literal: &[u8]) -> *mut c_void {
    let len = literal.len() as u64;
    let s = unsafe { __torajs_str_alloc_pooled(len) };
    if !literal.is_empty() {
        unsafe {
            memcpy(
                s.add(STR_HDR_SIZE) as *mut c_void,
                literal.as_ptr() as *const c_void,
                literal.len(),
            );
        }
    }
    s as *mut c_void
}

/// Allocate a `{status: string, value: number}` Obj. Mirrors the C
/// `alloc_settled_struct_` exactly: header(8) + class_tag(8 zeroed)
/// + vtable(8 zeroed) + status_ptr(8) + value(8) = 40 bytes.
unsafe fn alloc_settled_struct(state: u8, value: i64) -> *mut c_void {
    let p = unsafe { malloc(ALLSETTLED_OBJ_HEADER_SIZE + 16) } as *mut u8;
    unsafe {
        // Universal heap header.
        *(p as *mut u32) = 1;
        *(p.add(4) as *mut u16) = ALLSETTLED_OBJ_TAG;
        *(p.add(6) as *mut u16) = 0;
        // class_tag (+8) + vtable (+16) — zeroed for "no class".
        *(p.add(8) as *mut u64) = 0;
        *(p.add(16) as *mut u64) = 0;
        // status (+24)
        let lit = if state == STATE_FULFILLED {
            STATUS_FULFILLED_LIT
        } else {
            STATUS_REJECTED_LIT
        };
        let status_str = make_settled_str(lit);
        *(p.add(24) as *mut *mut c_void) = status_str;
        // value (+32)
        *(p.add(32) as *mut i64) = value;
    }
    p as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_allsettled_sync(
    promises_arr: *mut c_void,
) -> *mut c_void {
    if promises_arr.is_null() {
        return unsafe { __torajs_promise_alloc_rejected(0) };
    }
    let len = unsafe { arr_len(promises_arr) };
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        if pp.is_null() {
            continue;
        }
        if unsafe { (*pp).state } == STATE_PENDING {
            return unsafe { __torajs_promise_alloc_rejected(0) };
        }
    }
    let mut result_arr = unsafe { __torajs_arr_alloc(len) };
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        if pp.is_null() {
            let s = unsafe { alloc_settled_struct(STATE_REJECTED, 0) };
            result_arr = unsafe { __torajs_arr_push(result_arr, s as i64) };
            continue;
        }
        let s = unsafe { alloc_settled_struct((*pp).state, (*pp).value) };
        // T-17.c-A3 — heap-typed inner value: settled struct co-owns
        // it, so inc to pair the struct's eventual drop_heap call.
        unsafe {
            if (*pp).value_is_heap != 0 && (*pp).value != 0 {
                __torajs_rc_inc((*pp).value as *mut c_void);
            }
        }
        result_arr = unsafe { __torajs_arr_push(result_arr, s as i64) };
    }
    unsafe { __torajs_promise_alloc_fulfilled_heap(result_arr as i64) }
}

// ============================================================
// Promise.race — first settled wins
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_race_sync(promises_arr: *mut c_void) -> *mut c_void {
    if promises_arr.is_null() {
        return unsafe { __torajs_promise_alloc_rejected(0) };
    }
    let len = unsafe { arr_len(promises_arr) };
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        if pp.is_null() {
            continue;
        }
        let state = unsafe { (*pp).state };
        let value_is_heap = unsafe { (*pp).value_is_heap };
        let value = unsafe { (*pp).value };
        if state == STATE_FULFILLED {
            if value_is_heap != 0 {
                if value != 0 {
                    unsafe { __torajs_rc_inc(value as *mut c_void) };
                }
                return unsafe { __torajs_promise_alloc_fulfilled_heap(value) };
            }
            return unsafe { __torajs_promise_alloc_fulfilled(value) };
        }
        if state == STATE_REJECTED {
            if value_is_heap != 0 {
                if value != 0 {
                    unsafe { __torajs_rc_inc(value as *mut c_void) };
                }
                return unsafe { __torajs_promise_alloc_rejected_heap(value) };
            }
            return unsafe { __torajs_promise_alloc_rejected(value) };
        }
    }
    // Empty or all-pending — placeholder reject.
    unsafe { __torajs_promise_alloc_rejected(0) }
}

// ============================================================
// Promise.any — first FULFILLED wins; all-rejected → last reason
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_any_sync(promises_arr: *mut c_void) -> *mut c_void {
    if promises_arr.is_null() {
        return unsafe { __torajs_promise_alloc_rejected(0) };
    }
    let len = unsafe { arr_len(promises_arr) };
    let mut last_rejection: i64 = 0;
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        if pp.is_null() {
            continue;
        }
        let state = unsafe { (*pp).state };
        let value_is_heap = unsafe { (*pp).value_is_heap };
        let value = unsafe { (*pp).value };
        if state == STATE_FULFILLED {
            if value_is_heap != 0 {
                if value != 0 {
                    unsafe { __torajs_rc_inc(value as *mut c_void) };
                }
                return unsafe { __torajs_promise_alloc_fulfilled_heap(value) };
            }
            return unsafe { __torajs_promise_alloc_fulfilled(value) };
        }
        if state == STATE_REJECTED {
            last_rejection = value;
        }
    }
    unsafe { __torajs_promise_alloc_rejected(last_rejection) }
}
