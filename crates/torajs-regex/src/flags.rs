//! `RegExp` flag-string parsing — port of `runtime_regex.c`
//! L1389-1403.
//!
//! ECMA-262 §22.2.4.1.1 enumerates `i / g / m / s / u / y / d` as
//! valid flag letters; this port covers the first six (matching the
//! C source). Unknown bytes are silently skipped — JS would
//! SyntaxError, but the C port deferred that to Phase 1a stub-compat
//! behavior. L3b follow-up: strict mode that rejects unknowns at
//! `RegExp.compile` time.

use crate::parser::{RE_FLAG_G, RE_FLAG_I, RE_FLAG_M, RE_FLAG_S, RE_FLAG_U, RE_FLAG_Y};

pub fn parse_flags(s: &[u8]) -> u8 {
    let mut out = 0u8;
    for &b in s {
        match b {
            b'i' => out |= RE_FLAG_I,
            b'g' => out |= RE_FLAG_G,
            b'm' => out |= RE_FLAG_M,
            b's' => out |= RE_FLAG_S,
            b'u' => out |= RE_FLAG_U,
            b'y' => out |= RE_FLAG_Y,
            _ => {} // unknown — silently skip (matches C port)
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_flags_is_zero() {
        assert_eq!(parse_flags(b""), 0);
    }

    #[test]
    fn each_letter_sets_its_bit() {
        assert_eq!(parse_flags(b"i"), RE_FLAG_I);
        assert_eq!(parse_flags(b"g"), RE_FLAG_G);
        assert_eq!(parse_flags(b"m"), RE_FLAG_M);
        assert_eq!(parse_flags(b"s"), RE_FLAG_S);
        assert_eq!(parse_flags(b"u"), RE_FLAG_U);
        assert_eq!(parse_flags(b"y"), RE_FLAG_Y);
    }

    #[test]
    fn combined_flags_or_together() {
        let f = parse_flags(b"gim");
        assert_eq!(f, RE_FLAG_G | RE_FLAG_I | RE_FLAG_M);
    }

    #[test]
    fn all_six_flags_at_once() {
        let f = parse_flags(b"igmsuy");
        assert_eq!(
            f,
            RE_FLAG_I | RE_FLAG_G | RE_FLAG_M | RE_FLAG_S | RE_FLAG_U | RE_FLAG_Y
        );
    }

    #[test]
    fn duplicate_letters_idempotent() {
        // Per C-port behavior: duplicate flags don't error, just OR.
        assert_eq!(parse_flags(b"iii"), RE_FLAG_I);
        assert_eq!(parse_flags(b"gigig"), RE_FLAG_G | RE_FLAG_I);
    }

    #[test]
    fn unknown_letters_silently_skipped() {
        // `d` is in spec but not implemented; `z` doesn't exist.
        assert_eq!(parse_flags(b"d"), 0);
        assert_eq!(parse_flags(b"z"), 0);
        assert_eq!(parse_flags(b"izd"), RE_FLAG_I);
    }
}
