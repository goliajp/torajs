//! Locale identifier structural validation — ES402
//! IsStructurallyValidLanguageTag (§6.2.2) over the UTS #35
//! `unicode_locale_id` grammar. Consumed by the
//! `toLocale{Upper,Lower}Case` kernels ([`crate::transform::
//! case_locale`]) and the array-locales walk on the anyvalue side
//! (CanonicalizeLocaleList §9.2.1: every requested locale is
//! validated, RangeError on the first structural failure).
//!
//! Grammar implemented (structure only — no registry lookups, per
//! spec):
//!
//! ```text
//! unicode_locale_id     = unicode_language_id extensions* pu_extensions?
//! unicode_language_id   = language (- script)? (- region)? (- variant)*
//! language              = alpha{2,3} | alpha{5,8}
//! script                = alpha{4}
//! region                = alpha{2} | digit{3}
//! variant               = alphanum{5,8} | digit alphanum{3}
//! extension             = singleton (- alphanum{2,8})+     ; singleton != x
//! pu_extensions         = x (- alphanum{1,8})+
//! ```
//!
//! Plus the two spec-mandated uniqueness constraints: no duplicate
//! variant subtags, no duplicate singletons (all comparisons
//! ASCII-case-insensitive).

use alloc::vec::Vec;

#[inline]
fn is_alpha_n(s: &[u8], min: usize, max: usize) -> bool {
    s.len() >= min && s.len() <= max && s.iter().all(|b| b.is_ascii_alphabetic())
}

#[inline]
fn is_alphanum_n(s: &[u8], min: usize, max: usize) -> bool {
    s.len() >= min && s.len() <= max && s.iter().all(|b| b.is_ascii_alphanumeric())
}

#[inline]
fn is_variant(s: &[u8]) -> bool {
    is_alphanum_n(s, 5, 8)
        || (s.len() == 4 && s[0].is_ascii_digit() && is_alphanum_n(&s[1..], 4, 4))
}

#[inline]
fn eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x | 0x20) == (y | 0x20))
}

/// ES402 §6.2.2 — structural validity of a candidate BCP47 /
/// UTS #35 `unicode_locale_id`. Pure syntax; ASCII-case-insensitive.
pub(crate) fn is_structurally_valid_language_tag(s: &[u8]) -> bool {
    if s.is_empty() || !s.iter().all(|b| b.is_ascii()) {
        return false;
    }
    let parts: Vec<&[u8]> = s.split(|&b| b == b'-').collect();
    if parts.iter().any(|p| p.is_empty()) {
        return false;
    }
    let mut i = 1;
    // unicode_language_id — language subtag first. (UTS35 also
    // admits bare "root" / script-first ids; JSC — our parity
    // baseline — rejects both, matching the BCP47-shaped reading
    // most engines use, so tr rejects them too.)
    if is_alpha_n(parts[0], 2, 3) || is_alpha_n(parts[0], 5, 8) {
        if i < parts.len() && is_alpha_n(parts[i], 4, 4) {
            i += 1; // script
        }
        if i < parts.len()
            && (is_alpha_n(parts[i], 2, 2)
                || (parts[i].len() == 3 && parts[i].iter().all(|b| b.is_ascii_digit())))
        {
            i += 1; // region
        }
        let mut variants: Vec<&[u8]> = Vec::new();
        while i < parts.len() && is_variant(parts[i]) {
            if variants.iter().any(|v| eq_ignore_case(v, parts[i])) {
                return false;
            }
            variants.push(parts[i]);
            i += 1;
        }
    } else {
        return false;
    }
    // extensions + trailing private-use.
    let mut singletons: Vec<u8> = Vec::new();
    while i < parts.len() {
        if parts[i].len() != 1 || !parts[i][0].is_ascii_alphanumeric() {
            return false;
        }
        let sing = parts[i][0] | 0x20;
        i += 1;
        if sing == b'x' {
            // pu_extensions — must be last, at least one 1-8
            // alphanum subtag, consumes the rest of the tag.
            if i == parts.len() {
                return false;
            }
            while i < parts.len() {
                if !is_alphanum_n(parts[i], 1, 8) {
                    return false;
                }
                i += 1;
            }
            return true;
        }
        if singletons.contains(&sing) {
            return false;
        }
        singletons.push(sing);
        // extension subtags: at least one alphanum{2,8}, running
        // until the next singleton (or end).
        let mut count = 0;
        while i < parts.len() && parts[i].len() >= 2 && is_alphanum_n(parts[i], 2, 8) {
            count += 1;
            i += 1;
        }
        if count == 0 {
            return false;
        }
    }
    true
}

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
    if ascii && is_structurally_valid_language_tag(&bytes) {
        return 1;
    }
    let mut msg: Vec<u8> = b"invalid language tag: ".to_vec();
    msg.extend_from_slice(&bytes);
    msg.push(0);
    unsafe { __torajs_throw_range_error(msg.as_ptr()) };
    0
}

#[cfg(test)]
mod tests {
    use super::is_structurally_valid_language_tag as valid;

    #[test]
    fn accepts_common_tags() {
        for t in [
            "en",
            "tr",
            "az",
            "lt",
            "und",
            "en-US",
            "zh-Hans-CN",
            "de-DE",
            "TR-TR",
            "ca-ES-valencia",
            "sl-rozaj-biske",
            "de-DE-u-co-phonebk",
            "en-t-en",
            "en-US-x-priv",
            "en-a-bbb-x-a-yz",
            "hy-Latn-IT-arevela",
            "es-419",
        ] {
            assert!(valid(t.as_bytes()), "{t} should be valid");
        }
    }

    #[test]
    fn rejects_structural_failures() {
        for t in [
            "",
            "this is not a valid locale",
            "en-",
            "-en",
            "en--US",
            "a",
            "abcd",
            "en-US-US",       // duplicate-position region reads as bad variant
            "en-u",           // singleton with no subtags
            "en-u-co-u-nu",   // duplicate singleton
            "x-priv",         // private-use only
            "en-x",           // empty private-use
            "en-rozaj-rozaj", // duplicate variant
            "en-ROZAJ-rozaj", // duplicate variant, case-insensitive
            "i-klingon",      // legacy/grandfathered is not unicode_locale_id
            "root",           // UTS35 admits it; JSC (parity baseline) rejects
            "en-\u{00FF}",
        ] {
            assert!(!valid(t.as_bytes()), "{t:?} should be invalid");
        }
    }
}
