//! The FFI faces of locale-tag validation: the structural check
//! itself is [`crate::language_tag`] (ECMA-402 §6.2.1). Consumed by
//! the `toLocale{Upper,Lower}Case` kernels ([`crate::transform::
//! case_locale`]), the CanonicalizeLocaleList walk on the anyvalue side
//! (§9.2.1: every requested locale is validated, RangeError on the
//! first structural failure) and the typed lowering of a literal tag.

use alloc::vec::Vec;

use crate::language_tag::is_structurally_valid_language_tag;

unsafe extern "C" {
    /// `torajs-throw`'s cross-TU catchable `RangeError` raise (same
    /// extern `crate::normalize` declares).
    fn __torajs_throw_range_error(msg: *const u8);
}

/// FFI face shared by the typed-tier kernels and the cross-crate
/// array-locales walk (anyvalue side). Answers 1 when `locale` is a
/// structurally valid language tag; otherwise records a pending
/// catchable `RangeError` (bun-compatible `invalid language tag:
/// <tag>` message) and answers 0 — the caller echoes its receiver
/// as the stand-in and lets the call site's throw check kill the
/// path.
///
/// # Safety
///
/// `locale` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_locale_check(locale: *const u8) -> i64 {
    let (bytes, ascii) = unsafe { tag_bytes(locale) };
    if ascii && is_structurally_valid_language_tag(&bytes) {
        return 1;
    }
    raise_range_error(b"invalid language tag: ", &bytes, b"");
    0
}

/// Record the pending catchable `RangeError` for a tag already known
/// to be structurally invalid: the typed lowering emits this where the
/// `locales` argument is a string literal that fails ECMA-402 §6.2.1,
/// so the call needs no runtime check (CanonicalizeLocaleList §9.2.1
/// step 7.c.vi throws).
///
/// # Safety
///
/// `locale` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_locale_invalid(locale: *const u8) {
    let (bytes, _) = unsafe { tag_bytes(locale) };
    raise_range_error(b"invalid language tag: ", &bytes, b"");
}

/// Record `<prefix><tag><suffix>` as a pending catchable `RangeError`.
fn raise_range_error(prefix: &[u8], tag: &[u8], suffix: &[u8]) {
    let mut msg: Vec<u8> = prefix.to_vec();
    msg.extend_from_slice(tag);
    msg.extend_from_slice(suffix);
    msg.push(0);
    unsafe { __torajs_throw_range_error(msg.as_ptr()) };
}

/// The tag's code units as bytes, and whether all are ASCII.
///
/// # Safety
///
/// `locale` must be a valid Str heap block.
unsafe fn tag_bytes(locale: *const u8) -> (Vec<u8>, bool) {
    let (payload, length, is_latin1) = unsafe { crate::transform::case::str_view(locale) };
    let len = length as usize;
    let mut ascii = true;
    let mut bytes: Vec<u8> = Vec::with_capacity(len);
    for i in 0..len {
        let unit = if is_latin1 {
            payload[i] as u32
        } else {
            (payload[i * 2] as u32) | ((payload[i * 2 + 1] as u32) << 8)
        };
        if unit > 0x7F {
            ascii = false;
        }
        // Units above Latin-1 collapse lossily to `?` in the error
        // message — the valid grammar is ASCII-only, so only
        // already-invalid tags carry them.
        bytes.push(if unit > 0xFF { b'?' } else { unit as u8 });
    }
    (bytes, ascii)
}
