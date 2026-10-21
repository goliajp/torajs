//! `__torajs_regex_compile_baked` — the construction of a regex
//! literal. The compiler already compiled the literal at build time and
//! baked the result into the binary as an image ([`crate::image`]),
//! together with the one-pass matchers it synthesized and the DFA table
//! it built when the literal has them. Construction decodes the image,
//! so a program whose every RegExp is a literal links neither the
//! pattern parser nor the Unicode property tables nor the bytecode
//! compiler.

use core::ffi::c_void;
use core::ptr::NonNull;

use super::MatcherFn;
use super::compile::{Matchers, new_cell};
use super::str_units;
use crate::dfa::BakedDfaMeta;

/// # Safety
///
/// `image` is the 4-aligned image the compiler encoded for this very
/// literal, living for the process. `matcher_latin1` /
/// `matcher_utf16` are null or functions the compiler synthesized for
/// it; `meta` is null or the `BakedDfaMeta` it baked for it, whose
/// `states_ptr` points at `[DfaState; states_len]` living for the
/// process. `pattern_str` is the literal's live `Str *` source. The
/// returned pointer is a refcount-1 `RegExp`; release with
/// `__torajs_regex_drop`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_compile_baked(
    image: *const u8,
    matcher_latin1: Option<MatcherFn>,
    matcher_utf16: Option<MatcherFn>,
    meta: *const BakedDfaMeta,
    pattern_str: *const c_void,
) -> *mut c_void {
    let compiled = unsafe { crate::image::decode(image) };
    let src_bytes = unsafe { str_units(pattern_str) };
    let matchers = Matchers {
        latin1: matcher_latin1,
        utf16: matcher_utf16,
    };
    new_cell(
        compiled,
        src_bytes,
        NonNull::new(meta as *mut BakedDfaMeta),
        matchers,
    )
}
