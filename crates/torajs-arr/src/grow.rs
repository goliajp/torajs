//! Array growth + length-mutation helpers.
//!
//! This module gathers ops that change an array's `len` field — push /
//! reserve / shift (T-13.5 deque) + the spec-validator for `arr.length =
//! N` assignment. Sub-step matrix (P4.1):
//!
//! | Sub  | Adds                                                          |
//! |------|---------------------------------------------------------------|
//! | P4.1-j | `__torajs_arr_set_length_validate` (ES §9.4.2.4 guard)      |
//! | P4.1-k | `__torajs_arr_reserve` (cap doubling, port of IR builder)   |
//! | P4.1-l | `__torajs_arr_push` (typed push with auto-grow)             |
//! | P4.1-m | `__torajs_arr_shift` (T-13.5 deque head_offset fold)        |

unsafe extern "C" {
    /// Cross-tier — provided by torajs-throw. We don't depend on the
    /// crate (substrate-tier policy) — symbol resolves at `tr build`
    /// link time via `libtorajs_throw.a`.
    fn __torajs_throw_range_error(msg: *const u8) -> !;
}

/// `arr.length = v` validator (ES §9.4.2.4: throw RangeError if `v`
/// doesn't ToUint32-round-trip).
///
/// Tora's typed pack:
/// - tag 0 = null/other → ToNumber=0 → valid (early return)
/// - tag 1 = Bool 0/1 → valid (early return)
/// - tag 2 = I64 → interpret raw int as length candidate
/// - tag 3 = F64-bits → reinterpret raw bits as f64
/// - other = heap / undefined → throw immediately
///
/// Range check: `n` must be a non-negative integer in `[0, 2^32 - 1]`.
/// NaN, Infinity, fractional, negative, and overflow all fail.
///
/// # Safety
/// `extern "C"` ABI. Diverges (panics via throw_range_error) on invalid
/// input; caller must not assume return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_set_length_validate(tag: i64, value: i64) {
    let n: f64 = match tag {
        0 => return,
        1 => return,
        2 => value as f64,
        3 => f64::from_bits(value as u64),
        _ => unsafe {
            __torajs_throw_range_error(b"Invalid array length\0".as_ptr());
        },
    };
    // n.is_nan() || n < 0.0 || n > 2^32-1 || n is not an integer
    if n.is_nan() || n < 0.0 || n > 4_294_967_295.0 || n != (n as i64) as f64 {
        unsafe {
            __torajs_throw_range_error(b"Invalid array length\0".as_ptr());
        }
    }
}
