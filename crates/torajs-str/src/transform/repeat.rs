//! `s.repeat(n)` — split out of [`super::construct`], whose other
//! builders all carve or pick from one source; this one multiplies it.

use crate::block::StrBlock;
use crate::max_len::{__torajs_str_too_long, STR_MAX_LEN, str_len_of};

use super::construct::str_view;

unsafe extern "C" {
    /// `torajs-throw`'s cross-TU `RangeError` raise (pending throw in
    /// TLS; the call site's throw check propagates it).
    fn __torajs_throw_range_error(msg: *const u8);
}

/// Code units in `s.repeat(n)`: 0 when `n <= 0` or `s` is empty,
/// `None` past [`STR_MAX_LEN`].
#[inline]
pub fn repeat_out_len(s_len: u32, n: i64) -> Option<u32> {
    if n <= 0 || s_len == 0 {
        return Some(0);
    }
    // with `n` at most STR_MAX_LEN the product fits a usize
    if n as u64 > STR_MAX_LEN as u64 {
        return None;
    }
    str_len_of(s_len as usize * n as usize)
}

/// `s.repeat(n)` — fresh Str holding `s` concatenated `n` times.
/// `n == 0` yields the empty Str.
///
/// Per ES §22.1.3.16 steps 3-4, the spec throws `RangeError` when
/// `n < 0` or when `n` is `+Infinity`. The SSA arm lowers the
/// argument via `FpToSi` so floating-point Infinity reaches us as
/// `i64::MAX` (`fcvtzs +Inf` on aarch64 / `cvttsd2si` on x86_64);
/// we treat that as the Infinity sentinel and throw the same error
/// as the negative path. NaN coerces to `0` via the same
/// FpToSi rule, matching V8/JSC's "ToInteger(NaN) = 0 → empty
/// string" observable behavior.
///
/// `n == i64::MAX` from a legitimate user literal (extremely
/// unlikely — the byte-cost would overflow `u32` anyway) gets
/// routed to the throw path; bun / V8 also raise on such inputs
/// because the resulting string can't be allocated.
///
/// The message matches bun's so conformance diff-tests can rely
/// on `(e as Error).message`.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_repeat(s: *const u8, n: i64) -> *mut u8 {
    if n < 0 || n == i64::MAX {
        unsafe {
            __torajs_throw_range_error(
                c"String.prototype.repeat argument must be greater than or equal to 0 and not be Infinity"
                    .as_ptr().cast(),
            )
        };
        // Caller's emit_throw_check picks up the TLS pending throw
        // immediately on return. The empty Str result is unreachable
        // in user code but keeps the SSA Call result type stable.
        let block = StrBlock::alloc_with_encoding(0, true);
        return block.into_raw();
    }
    let (s_payload, s_length, is_latin1) = unsafe { str_view(s) };
    let Some(out_length) = repeat_out_len(s_length, n) else {
        return unsafe { __torajs_str_too_long() };
    };
    let mut block = StrBlock::alloc_with_encoding(out_length, is_latin1);
    if out_length == 0 || s_length == 0 {
        return block.into_raw();
    }
    let s_byte_cnt = s_payload.len();
    let out_byte_cnt = if is_latin1 {
        out_length as usize
    } else {
        (out_length as usize) * 2
    };
    let dst = unsafe { block.as_bytes_mut(out_byte_cnt as u32) };
    let times = (out_length / s_length) as usize;
    for k in 0..times {
        dst[k * s_byte_cnt..(k + 1) * s_byte_cnt].copy_from_slice(s_payload);
    }
    block.into_raw()
}
