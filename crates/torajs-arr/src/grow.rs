//! Array growth + length-mutation helpers.
//!
//! This module gathers ops that change an array's `len` field — push /
//! reserve / shift (T-13.5 deque) + the spec-validator for `arr.length =
//! N` assignment. Sub-step matrix (P4.1):
//!
//! | Sub  | Adds                                                          |
//! |------|---------------------------------------------------------------|
//! | P4.1-j | `__torajs_arr_set_length_validate` (ES §9.4.2.4 guard)      |
//! | P4.1-k | `__torajs_arr_reserve` (realloc-if-cap-too-small)           |
//! | P4.1-l | `__torajs_arr_push` (typed push with auto-grow)             |
//! | P4.1-m | `__torajs_arr_shift` (T-13.5 deque head_offset fold)        |

use core::ffi::c_void;

/// Offset of the `cap` u32 within an array heap block. T-13.5 packed
/// cap (u32) + head_offset (u32) into the 8-byte slot at offset 16
/// (formerly cap was a u64). Mirrors `ssa_inkwell::ARR_HDR_CAP_OFF`.
const ARR_HDR_CAP_OFF: usize = 16;

/// Offset of the slot array within an array heap block (24 = 8B header
/// + 8B len + 4B cap + 4B head). Mirrors `ssa_inkwell::ARR_HDR_DATA_OFF`.
const ARR_HDR_DATA_OFF: usize = 24;

unsafe extern "C" {
    /// Cross-tier — provided by torajs-throw at `tr build` link time
    /// via `libtorajs_throw.a`.
    ///
    /// **Returns normally** — does NOT longjmp / panic. Internally
    /// records the pending throw via TLS (`__torajs_throw_set`). The
    /// caller's SSA-emitted `emit_throw_check` after our `return` is
    /// what actually propagates to user-side `try/catch`.
    fn __torajs_throw_range_error(msg: *const u8);

    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;
}

/// `arr.length = v` validator (ES §9.4.2.4: throw RangeError if `v`
/// doesn't ToUint32-round-trip).
///
/// Tora's typed pack:
/// - tag 0 = null/other → ToNumber=0 → valid (early return)
/// - tag 1 = Bool 0/1 → valid (early return)
/// - tag 2 = I64 → interpret raw int as length candidate
/// - tag 3 = F64-bits → reinterpret raw bits as f64
/// - other = heap / undefined → record RangeError + return
///
/// Range check: `n` must be a non-negative integer in `[0, 2^32 - 1]`.
/// NaN, Infinity, fractional, negative, and overflow all fail.
///
/// After every `__torajs_throw_range_error` call we `return` so the
/// caller's `emit_throw_check` sees the pending throw immediately (the
/// throw is non-local via TLS, not via stack unwind — see fn-level
/// extern doc).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_set_length_validate(tag: i64, value: i64) {
    let n: f64 = match tag {
        0 | 1 => return,
        2 => value as f64,
        3 => f64::from_bits(value as u64),
        _ => {
            unsafe {
                __torajs_throw_range_error(b"Invalid array length\0".as_ptr());
            }
            return;
        }
    };
    if n.is_nan() || n < 0.0 || n > 4_294_967_295.0 || n != (n as i64) as f64 {
        unsafe {
            __torajs_throw_range_error(b"Invalid array length\0".as_ptr());
        }
    }
}

/// Grow an array's backing block to fit at least `new_cap` elements.
/// Cap-equal short-circuits to no-op (returns input pointer unchanged).
///
/// **Returns the (possibly relocated) array pointer** — the caller
/// must use the return value, not the input pointer, since `realloc`
/// may move the block.
///
/// Algorithm (1:1 port of ssa_inkwell::define_arr_reserve, 66 LOC IR
/// → ~10 LOC Rust thanks to native realloc + raw-pointer arithmetic):
///
/// ```text
/// if cap(arr) >= new_cap: return arr   // no-op short-circuit
/// new_total = new_cap * 8 + ARR_HDR_DATA_OFF
/// arr = realloc(arr, new_total)
/// *(u32*)(arr + ARR_HDR_CAP_OFF) = new_cap as u32
/// return arr
/// ```
///
/// # Safety
/// `extern "C"` ABI. `arr` must be a live array heap block (non-NULL,
/// allocated via `__torajs_arr_alloc*`); `new_cap` non-negative.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_reserve(arr: *mut u8, new_cap: i64) -> *mut u8 {
    let cap_p = unsafe { arr.add(ARR_HDR_CAP_OFF) as *mut u32 };
    let cap = unsafe { *cap_p } as i64;
    if cap >= new_cap {
        return arr;
    }
    let new_total = (new_cap as usize) * 8 + ARR_HDR_DATA_OFF;
    let arr_grown = unsafe { realloc(arr as *mut c_void, new_total) as *mut u8 };
    unsafe {
        *(arr_grown.add(ARR_HDR_CAP_OFF) as *mut u32) = new_cap as u32;
    }
    arr_grown
}
