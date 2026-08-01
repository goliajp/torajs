//! §22.2.3.1 — `new RegExp(pattern, flags)` with runtime-shaped
//! (boxed Any) operands (rotation 267; the `RegExp(...)` call →
//! construct rewrite exposed non-Str patterns that the Str-shaped
//! entry read as Str cells — a RegExp-object pattern SIGSEGV'd).
//!
//! A RegExp pattern copies its source, and its flags when the flags
//! argument is undefined (step 5); an undefined pattern is the empty
//! pattern; everything else runs ToString (step 6). A pending
//! ToString throw answers a stub cell for the caller's owned
//! throw-check to release. A rejected pattern records the
//! §22.2.3.1 SyntaxError exactly like the Str-shaped entry.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::compile::{compile_bytes, flag_letters, throw_if_rejected};
use super::str_helpers::str_slice;
use super::subclass::pattern_as_regex;

/// `AnySlotTag::Undefined` mirror.
const ANY_UNDEF_TAG: i64 = 5;

unsafe extern "C" {
    /// torajs-anyvalue — NaN-box tag probe + spec ToString.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    fn __torajs_str_drop(s: *mut c_void);
}

/// See module doc. Answers a fresh owned RegExp cell in every case
/// (stub on a pending throw — the caller's `emit_throw_check_owned`
/// releases it on the divert path).
///
/// # Safety
/// `pat_av` / `flags_av` are boxed AnyValues; cell payloads are live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_compile_any(pat_av: u64, flags_av: u64) -> *mut c_void {
    unsafe {
        let explicit_flags: Option<Vec<u8>> = if __torajs_anyv_unbox_tag(flags_av) == ANY_UNDEF_TAG
        {
            None
        } else {
            let s = __torajs_anyv_to_str(flags_av);
            if s.is_null() {
                return stub();
            }
            let b = str_slice(s);
            __torajs_str_drop(s);
            Some(b)
        };
        let (pat, re_flags) = if __torajs_anyv_unbox_tag(pat_av) == ANY_UNDEF_TAG {
            (b"(?:)".to_vec(), Vec::new())
        } else if let Some(re) = pattern_as_regex(pat_av) {
            ((*re).src_bytes.clone(), flag_letters((*re).flags))
        } else {
            let s = __torajs_anyv_to_str(pat_av);
            if s.is_null() {
                return stub();
            }
            let b = str_slice(s);
            __torajs_str_drop(s);
            (b, Vec::new())
        };
        // Step 5 — flags default to the RegExp pattern's own when the
        // flags argument is undefined.
        let fl = explicit_flags.unwrap_or(re_flags);
        let raw = compile_bytes(pat, fl);
        throw_if_rejected(raw);
        raw
    }
}

/// Throw-shape answer — a live stub cell the caller's owned
/// throw-check releases; the pending throw is already recorded by
/// the ToString that failed.
fn stub() -> *mut c_void {
    compile_bytes(b"(?:)".to_vec(), Vec::new())
}
