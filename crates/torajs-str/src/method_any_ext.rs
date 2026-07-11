//! `any`-receiver String method glue, second slice — pad / repeat /
//! concat / codePointAt / localeCompare / normalize / lastIndexOf /
//! search / matchAll. Split out of [`crate::method_any`] by the
//! 500-line file discipline; same contract (raw AnyValue bits out,
//! fresh rc=1 cells transfer to the caller) and the same
//! [`owned_src`] Substr-materialize pattern.

use crate::block::__torajs_str_alloc;
use crate::method_any::{KIND_HEAP_CHAIN, drop_tmp, owned_src};

unsafe extern "C" {
    /// Cross-tier — torajs-arr element-kind stamp (see
    /// [`crate::method_any`]'s decl).
    fn __torajs_arr_mark_kind(arr: *mut core::ffi::c_void, chain: u64);
    /// Cross-tier — torajs-regex kernels (owned-Str layout inputs).
    fn __torajs_str_search_regex(s: *const core::ffi::c_void, re: *const core::ffi::c_void) -> i64;
    fn __torajs_str_match_all_regex(
        s: *const core::ffi::c_void,
        re: *const core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
}

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

/// `s.normalize(form)` per ES §22.1.3.14 — NULL `form` denotes a
/// missing argument (the spec "NFC" default). The kernel records a
/// pending RangeError for an invalid form and echoes the receiver
/// as its stand-in; that echo must not transfer to the caller (it
/// would double-free the receiver or dangle a materialized temp),
/// so the throw path substitutes a fresh empty Str — the call
/// site's throw check kills the path before the value is consumed.
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer; `form` is NULL or one
/// too.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_normalize(s: *const u8, form: *const u8) -> u64 {
    unsafe {
        let (src, s_tmp) = owned_src(s);
        let (fm, f_tmp) = if form.is_null() {
            let nfc = __torajs_str_alloc(b"NFC".as_ptr(), 3);
            (nfc as *const u8, nfc as *mut u8)
        } else {
            owned_src(form)
        };
        let out = crate::normalize::__torajs_str_normalize(src, fm);
        let out = if out == src as *mut u8 {
            __torajs_str_alloc(core::ptr::null(), 0)
        } else {
            out
        };
        drop_tmp(f_tmp);
        drop_tmp(s_tmp);
        out as u64
    }
}

/// `s.lastIndexOf(needle, from)` per ES §22.1.3.11 — found
/// code-unit index or `-1`. A missing / NaN `from` rides as
/// `i64::MAX` (the kernel clamps to the last viable start).
///
/// # Safety
/// `s` and `needle` are valid heap Str/Substr pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_last_index_of(
    s: *const u8,
    needle: *const u8,
    from: i64,
) -> i64 {
    unsafe {
        let (src, s_tmp) = owned_src(s);
        let (nn, n_tmp) = owned_src(needle);
        let out = crate::lookup_ffi::__torajs_str_last_index_of_from(src, nn, from);
        drop_tmp(n_tmp);
        drop_tmp(s_tmp);
        out
    }
}

/// `s.search(re)` per ES §22.1.3.20 — match start in code units or
/// `-1`.
///
/// # Safety
/// `s` is a live Str/Substr cell; `re` is a live RegExp cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_search(
    s: *const u8,
    re: *const core::ffi::c_void,
) -> i64 {
    unsafe {
        let (src, tmp) = owned_src(s);
        let out = __torajs_str_search_regex(src as *const core::ffi::c_void, re);
        drop_tmp(tmp);
        out
    }
}

/// `s.matchAll(re)` per ES §22.1.3.13 — array of exec-shape match
/// arrays, heap-chain-marked for any-world reads at BOTH levels
/// (the outer product and each inner match array — its slots are
/// the matched Str fragments). The kernel records a pending
/// TypeError for a non-global regex (and answers an empty array the
/// throw check discards).
///
/// # Safety
/// `s` is a live Str/Substr cell; `re` is a live RegExp cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_match_all(
    s: *const u8,
    re: *const core::ffi::c_void,
) -> u64 {
    // torajs-arr cell layout mirror (`layout.rs`, B1 fixed cell) —
    // same cross-crate sync posture as `split/pool.rs`.
    const ARR_LEN_OFF: usize = 8;
    const ARR_DATA_PTR_OFF: usize = 32;
    unsafe {
        let (src, tmp) = owned_src(s);
        let out = __torajs_str_match_all_regex(src as *const core::ffi::c_void, re);
        drop_tmp(tmp);
        __torajs_arr_mark_kind(out, KIND_HEAP_CHAIN);
        let cell = out as *const u8;
        let len = (cell.add(ARR_LEN_OFF) as *const u64).read();
        let data = (cell.add(ARR_DATA_PTR_OFF) as *const *const u64).read();
        for i in 0..len as usize {
            let inner = data.add(i).read() as *mut core::ffi::c_void;
            if !inner.is_null() {
                __torajs_arr_mark_kind(inner, KIND_HEAP_CHAIN);
            }
        }
        out as u64
    }
}
