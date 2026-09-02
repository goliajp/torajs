//! `RegExp` flag-string parsing — ES §22.2.4.1.1 RegExp Constructor
//! flags string validation.
//!
//! Eight valid flag letters (`d / g / i / m / s / u / v / y`).
//! Duplicate letters and unknown letters are both Early Errors per
//! §22.2.3.1 RegExp Runtime Semantics — `parse_flags` reports either
//! by returning `None`. The `u` + `v` combined-flag rejection lives
//! at the caller (each `regex_compile*` entry does its own
//! `flag_conflict` check) so that surface is unchanged.

use crate::parser::{
    RE_FLAG_D, RE_FLAG_G, RE_FLAG_I, RE_FLAG_M, RE_FLAG_S, RE_FLAG_U, RE_FLAG_V, RE_FLAG_Y,
};

/// Strict flag-string parse. `None` = SyntaxError (unknown or
/// duplicate letter). Callers that need to raise a JS SyntaxError
/// treat `None` as "rejected" — `regex_compile` routes it through
/// the same never-match stub path as a malformed pattern so
/// `__torajs_regex_compile_or_throw` records the pending throw.
/// `u` or `v`: the pattern and the haystack are code-point
/// sequences; without either they are UTF-16 code-unit sequences
/// (§22.2.2.1 — `.` and a class step one unit, a surrogate pair is
/// two of them).
pub fn unicode_mode(flags: u8) -> bool {
    flags & (RE_FLAG_U | RE_FLAG_V) != 0
}

pub fn parse_flags(s: &[u8]) -> Option<u8> {
    let mut out = 0u8;
    for &b in s {
        let bit = match b {
            b'i' => RE_FLAG_I,
            b'g' => RE_FLAG_G,
            b'm' => RE_FLAG_M,
            b's' => RE_FLAG_S,
            b'u' => RE_FLAG_U,
            b'y' => RE_FLAG_Y,
            b'v' => RE_FLAG_V,
            b'd' => RE_FLAG_D,
            // Unknown letter — SyntaxError per §22.2.3.1.
            _ => return None,
        };
        // Duplicate letter — SyntaxError per §22.2.3.1.
        if out & bit != 0 {
            return None;
        }
        out |= bit;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_flags_is_some_zero() {
        assert_eq!(parse_flags(b""), Some(0));
    }

    #[test]
    fn each_letter_sets_its_bit() {
        assert_eq!(parse_flags(b"i"), Some(RE_FLAG_I));
        assert_eq!(parse_flags(b"g"), Some(RE_FLAG_G));
        assert_eq!(parse_flags(b"m"), Some(RE_FLAG_M));
        assert_eq!(parse_flags(b"s"), Some(RE_FLAG_S));
        assert_eq!(parse_flags(b"u"), Some(RE_FLAG_U));
        assert_eq!(parse_flags(b"y"), Some(RE_FLAG_Y));
        assert_eq!(parse_flags(b"d"), Some(RE_FLAG_D));
        assert_eq!(parse_flags(b"v"), Some(RE_FLAG_V));
    }

    #[test]
    fn combined_flags_or_together() {
        let f = parse_flags(b"gim").expect("gim");
        assert_eq!(f, RE_FLAG_G | RE_FLAG_I | RE_FLAG_M);
    }

    #[test]
    fn all_eight_flags_at_once_minus_uv_conflict() {
        // u and v are mutually exclusive at the caller layer, but
        // parse_flags itself doesn't police the combo — a fully
        // spelled `digmsuy` is well-formed here (7 letters, no dup).
        let f = parse_flags(b"digmsuy").expect("digmsuy");
        assert_eq!(
            f,
            RE_FLAG_D | RE_FLAG_I | RE_FLAG_G | RE_FLAG_M | RE_FLAG_S | RE_FLAG_U | RE_FLAG_Y
        );
    }

    #[test]
    fn duplicate_letters_reject() {
        assert_eq!(parse_flags(b"gg"), None);
        assert_eq!(parse_flags(b"iii"), None);
        assert_eq!(parse_flags(b"gigig"), None);
        // Duplicate anywhere in the string rejects, not just adjacent.
        assert_eq!(parse_flags(b"igmi"), None);
    }

    #[test]
    fn unknown_letters_reject() {
        assert_eq!(parse_flags(b"z"), None);
        assert_eq!(parse_flags(b"izq"), None);
        // Digit / punctuation reject too.
        assert_eq!(parse_flags(b"i1"), None);
        assert_eq!(parse_flags(b"g,"), None);
    }
}
