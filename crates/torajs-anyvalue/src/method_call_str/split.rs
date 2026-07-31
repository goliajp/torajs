//! `String.prototype.split` any-separator runtime family —
//! §22.1.3.23 step 4-onward kernels shared by the ANY_METHOD_SPLIT
//! arm and the typed-receiver `@@split` probe-miss lane (the step-2
//! splitter dispatch lives with the callers: the arm probes inline,
//! the typed lane branches in `ssa_lower_call_str_match_custom`).
//! Moved verbatim out of the parent when the `@@split` twin pushed
//! it past the 500-line cap (rotation 264).

use core::ffi::c_void;

use super::{
    __torajs_anyv_box_pointer, __torajs_anyv_to_number, __torajs_anyv_to_str,
    __torajs_arr_alloc_any, __torajs_arr_any_slice, __torajs_str_any_split,
    __torajs_str_any_split_regex, __torajs_str_drop, __torajs_throw_check,
    __torajs_value_drop_heap, regexp_cell,
};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_undefined};

/// §22.1.3.23 step 2 — the three-way separator dispatch shared by
/// the ANY_METHOD_SPLIT arm and the typed-receiver `Any`-separator
/// lane: undefined skips splitting ([S]), a RegExp cell hands off
/// to `@@split`, everything else ToStrings.
///
/// # Safety
/// `s` is a live Str/Substr cell; `sep_av` carries a valid AnyValue
/// bit pattern.
pub(super) unsafe fn split_with_any_sep(s: *mut u8, sep_av: u64) -> AnyValue {
    unsafe {
        if is_undefined(sep_av) {
            __torajs_str_any_split(s, core::ptr::null())
        } else if let Some(re) = regexp_cell(sep_av) {
            // The prior ToString lane matched the literal "/pat/"
            // spelling instead (test262 15.5.4.14 A4 family — every
            // `new String(s).split(regexp)` answered one unsplit
            // token).
            __torajs_str_any_split_regex(s, re)
        } else {
            let sep = __torajs_anyv_to_str(sep_av);
            let out = __torajs_str_any_split(s, sep as *const u8);
            __torajs_str_drop(sep);
            out
        }
    }
}

/// §7.1.6 ToUint32 for the split limit — undefined never reaches
/// here (the caller rides the no-limit path); NaN / ±∞ answer 0,
/// finite values truncate toward zero then wrap mod 2^32.
unsafe fn split_lim(limit_av: AnyValue) -> i64 {
    let n = unsafe { __torajs_anyv_to_number(limit_av) };
    if !n.is_finite() {
        return 0;
    }
    n.trunc().rem_euclid(4294967296.0) as i64
}

/// §22.1.3.23 steps 4-9 for the any-receiver lane with a present
/// limit argument — lim's ToUint32 runs BEFORE any separator
/// coercion (step 4 precedes steps 5-7: lim == 0 answers `[]`
/// without ToString-ing the separator, which the
/// separator-override-tostring test262 family pins), then the split
/// product truncates to its first `lim` tokens. A RegExp separator's
/// `@@split` collector stops at lim per RegExpSplit steps 13-19, so
/// prefix-truncating the full product is observationally equal.
///
/// # Safety
/// `s` is a live Str/Substr cell; `sep_av` / `limit_av` carry valid
/// AnyValue bit patterns.
pub(super) unsafe fn split_any_with_limit(s: *mut u8, sep_av: u64, limit_av: u64) -> AnyValue {
    unsafe {
        let lim = split_lim(limit_av);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        if lim == 0 {
            return __torajs_anyv_box_pointer(__torajs_arr_alloc_any(0) as *mut c_void);
        }
        let full = split_with_any_sep(s, sep_av);
        let sliced = __torajs_arr_any_slice(full as *const u8, 0, lim);
        __torajs_value_drop_heap(full as *mut c_void);
        sliced as u64
    }
}

/// Typed-receiver `s.split(sep)` with an `any` separator — the SSA
/// lowering's static undefined guards cannot see through an As-cast
/// or an `any` binding, so the (Str, Str) kernel used to receive
/// raw AnyValue bits as a pointer (SIGSEGV). One runtime entry runs
/// the same three-way dispatch the any lane uses; the product is an
/// Arr cell pointer either way, so the typed limit-clamp slice
/// downstream applies unchanged.
///
/// # Safety
/// `s` is a live Str/Substr cell; `sep_av` carries a valid AnyValue
/// bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_split_any_sep(s: *mut u8, sep_av: u64) -> u64 {
    unsafe { split_with_any_sep(s, sep_av) }
}

/// The limit-carrying twin for the typed-receiver `@@split` miss
/// lane — the limit arrives as a RAW AnyValue (undefined = absent)
/// because §22.1.3.23 step 4's ToUint32 belongs to this fallback,
/// not to the step-2 splitter dispatch the caller already probed.
///
/// # Safety
/// `s` is a live Str/Substr cell; `sep_av` / `limit_av` carry valid
/// AnyValue bit patterns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_split_any_sep_lim(
    s: *mut u8,
    sep_av: u64,
    limit_av: u64,
) -> u64 {
    unsafe {
        if is_undefined(limit_av) {
            split_with_any_sep(s, sep_av)
        } else {
            split_any_with_limit(s, sep_av, limit_av)
        }
    }
}
