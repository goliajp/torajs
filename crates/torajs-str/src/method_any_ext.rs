//! `any`-receiver String method glue, second slice — pad / repeat /
//! concat / codePointAt / localeCompare. Split out of
//! [`crate::method_any`] by the 500-line file discipline; same
//! contract (raw AnyValue bits out, fresh rc=1 cells transfer to the
//! caller) and the same [`owned_src`] Substr-materialize pattern.

use crate::block::__torajs_str_alloc;
use crate::method_any::{drop_tmp, owned_src};

/// `s.padStart(targetLen, padStr)` / `padEnd` per ES §22.1.3.17/16 —
/// `end != 0` picks padEnd. NULL `pad` denotes a missing pad
/// argument (the spec default is a single space); the empty-pad and
/// target-too-small passthroughs live in the kernel.
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer; `pad` is NULL or one too.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_pad(
    s: *const u8,
    target_len: i64,
    pad: *const u8,
    end: i64,
) -> u64 {
    unsafe {
        let (src, s_tmp) = owned_src(s);
        let (pd, pad_tmp) = if pad.is_null() {
            let sp = __torajs_str_alloc(b" ".as_ptr(), 1);
            (sp as *const u8, sp as *mut u8)
        } else {
            owned_src(pad)
        };
        let out = if end != 0 {
            crate::transform::pad::__torajs_str_pad_end(src, target_len, pd)
        } else {
            crate::transform::pad::__torajs_str_pad_start(src, target_len, pd)
        };
        drop_tmp(pad_tmp);
        drop_tmp(s_tmp);
        out as u64
    }
}

/// `s.repeat(n)` per ES §22.1.3.19 — the kernel records the
/// RangeError (negative / Infinity-sentinel `n`) as a TLS pending
/// throw and still returns a placeholder cell; the dispatcher's
/// throw-check propagation picks it up on return.
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_repeat(s: *const u8, n: i64) -> u64 {
    unsafe {
        let (src, tmp) = owned_src(s);
        let out = crate::transform::construct::__torajs_str_repeat(src, n);
        drop_tmp(tmp);
        out as u64
    }
}

/// One `s.concat(...)` fold step per ES §22.1.3.5 — the dispatcher
/// ToStrings each argument and folds left through this pairwise
/// kernel wrap.
///
/// # Safety
/// `a` and `b` are valid heap Str/Substr pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_concat2(a: *const u8, b: *const u8) -> u64 {
    unsafe {
        let (aa, a_tmp) = owned_src(a);
        let (bb, b_tmp) = owned_src(b);
        let out = crate::concat::__torajs_str_concat(aa, bb);
        drop_tmp(b_tmp);
        drop_tmp(a_tmp);
        out as u64
    }
}

/// `s.codePointAt(i)` per ES §22.1.3.4 — the code point (surrogate
/// pairs combine), or `-1` for out-of-range (the dispatcher boxes
/// that as the spec `undefined`; the typed-tier kernel's 0-for-OOB
/// stays its own recorded divergence).
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_code_point_at(s: *const u8, i: i64) -> i64 {
    unsafe {
        let (src, tmp) = owned_src(s);
        let (_, len, _) = crate::lookup::str_view(src);
        let out = if i < 0 || i >= len as i64 {
            -1
        } else {
            crate::code_point::__torajs_str_code_point_at(src, i)
        };
        drop_tmp(tmp);
        out
    }
}

/// `s.localeCompare(other)` per ES §22.1.3.12 — `-1` / `0` / `1`
/// byte-lexicographic order (the typed tier's locale posture).
///
/// # Safety
/// `s` and `other` are valid heap Str/Substr pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_locale_compare(s: *const u8, other: *const u8) -> i64 {
    unsafe {
        let (aa, a_tmp) = owned_src(s);
        let (bb, b_tmp) = owned_src(other);
        let out = crate::lookup_ffi::__torajs_str_locale_compare(aa, bb);
        drop_tmp(b_tmp);
        drop_tmp(a_tmp);
        out
    }
}
