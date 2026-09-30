//! `__torajs_regex_compile_from_static_matcher` — the compile entry
//! for a literal whose one-pass matcher the compiler emitted as a
//! native function. The RegExp is the one the runtime entries build
//! (`__torajs_regex_compile`, or the baked-DFA entry when the literal
//! also carries a table), plus the matcher pointers the exec kernels
//! take before the transcoding search.

use core::ffi::c_void;

use super::MatcherFn;
use super::as_regex_mut;
use super::compile::__torajs_regex_compile;
use super::compile_aot::__torajs_regex_compile_from_static_dfa;
use crate::dfa::BakedDfaMeta;
use crate::vm::saves_arena::detect_stride;

/// # Safety
///
/// `matcher_latin1` / `matcher_utf16` are null or functions the
/// compiler synthesized for this very pattern and flags; `meta` is
/// null or a `BakedDfaMeta` under the contract of
/// [`__torajs_regex_compile_from_static_dfa`]; `pattern_str` and
/// `flags_str` are live `Str *` cells. The returned pointer is a
/// refcount-1 `RegExp`; release with `__torajs_regex_drop`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_compile_from_static_matcher(
    matcher_latin1: Option<MatcherFn>,
    matcher_utf16: Option<MatcherFn>,
    meta: *const BakedDfaMeta,
    pattern_str: *const c_void,
    flags_str: *const c_void,
) -> *mut c_void {
    let raw = if meta.is_null() {
        unsafe { __torajs_regex_compile(pattern_str, flags_str) }
    } else {
        unsafe { __torajs_regex_compile_from_static_dfa(meta, pattern_str, flags_str) }
    };
    let re = unsafe { as_regex_mut(raw) };
    re.matcher_latin1 = matcher_latin1;
    re.matcher_utf16 = matcher_utf16;
    re.matcher_stride = detect_stride(&re.prog) as u32;
    raw
}
