//! Char class — port of `runtime_regex.c` L254-349.
//!
//! 256-bit ASCII bitmap + inversion bit + Unicode property bitfield.
//! One per `OP_CLASS` instruction in the future Program (interned by
//! `Program.classes[]` in P6.2-c).
//!
//! ASCII portion lives in [`CharClass::bits`]; under the u flag, the
//! cp ≥ 128 portion is covered by the curated UCD tables in
//! [`super::ucd`] and consulted via [`CharClass::test_cp`].

use crate::ucd::{UCD_LETTER, UCD_NUMBER, UP_LETTER, UP_NUMBER, uprop_range_contains};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharClass {
    pub bits: [u8; 32],
    pub negate: bool,
    /// Unicode property bitfield. When set (via `\p{NAME}` in a u-flag
    /// pattern), [`test_cp`](Self::test_cp) consults the static UCD
    /// tables for `cp ≥ 128`. ASCII portion of each property lives in
    /// `bits` (populated by `add_property_*`). Class-level `negate`
    /// still applies after the union.
    pub u_props: u8,
}

impl Default for CharClass {
    fn default() -> Self {
        Self::new()
    }
}

impl CharClass {
    pub const fn new() -> Self {
        Self {
            bits: [0; 32],
            negate: false,
            u_props: 0,
        }
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    pub fn add(&mut self, ch: u8) {
        self.bits[(ch >> 3) as usize] |= 1u8 << (ch & 7);
    }

    pub fn add_range(&mut self, lo: u8, hi: u8) {
        let (lo, hi) = if lo > hi { (hi, lo) } else { (lo, hi) };
        for c in lo..=hi {
            self.add(c);
        }
    }

    pub fn test(&self, ch: u8) -> bool {
        let in_set = (self.bits[(ch >> 3) as usize] >> (ch & 7)) & 1 != 0;
        if self.negate { !in_set } else { in_set }
    }

    /// Code-point membership test for the u flag — port of `cc_test_cp`.
    ///
    /// `cp < 128` is bitmap-tested. `cp ≥ 128` with no `u_props` set is
    /// a miss (bitmap doesn't reach there). `cp ≥ 128` with `u_props`
    /// bits set scans the curated UCD tables. Class-level `negate`
    /// inverts after the union.
    pub fn test_cp(&self, cp: i32) -> bool {
        let mut in_set = false;
        if (0..256).contains(&cp) {
            in_set = (self.bits[(cp >> 3) as usize] >> (cp & 7)) & 1 != 0;
        }
        if !in_set && self.u_props != 0 && cp >= 0x80 {
            if self.u_props & UP_LETTER != 0 && uprop_range_contains(UCD_LETTER, cp) {
                in_set = true;
            } else if self.u_props & UP_NUMBER != 0 && uprop_range_contains(UCD_NUMBER, cp) {
                in_set = true;
            }
        }
        if self.negate { !in_set } else { in_set }
    }

    // Predefined-class helpers — \d, \w, \s.
    pub fn add_digit(&mut self) {
        self.add_range(b'0', b'9');
    }

    pub fn add_word(&mut self) {
        self.add_range(b'0', b'9');
        self.add_range(b'A', b'Z');
        self.add_range(b'a', b'z');
        self.add(b'_');
    }

    /// `\s` — ECMA-262 whitespace, ASCII subset (space, tab, LF, VT, FF, CR).
    pub fn add_space(&mut self) {
        for &c in b" \t\n\x0b\x0c\r" {
            self.add(c);
        }
    }

    /// `\p{L}` / `\p{Letter}` — ASCII portion + UCD Letter ranges.
    pub fn add_property_letter(&mut self) {
        self.add_range(b'A', b'Z');
        self.add_range(b'a', b'z');
        self.u_props |= UP_LETTER;
    }

    /// `\p{N}` / `\p{Number}` — ASCII digits + UCD Number ranges.
    pub fn add_property_number(&mut self) {
        self.add_range(b'0', b'9');
        self.u_props |= UP_NUMBER;
    }

    /// `\p{ASCII}` — `[\x00-\x7F]`. No `u_props` bit needed (cp ≥ 128
    /// never matches ASCII regardless of bits set).
    pub fn add_property_ascii(&mut self) {
        for c in 0..=0x7Fu8 {
            self.add(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_test_single_byte() {
        let mut cc = CharClass::new();
        cc.add(b'A');
        assert!(cc.test(b'A'));
        assert!(!cc.test(b'B'));
    }

    #[test]
    fn add_range_inclusive_endpoints() {
        let mut cc = CharClass::new();
        cc.add_range(b'a', b'c');
        assert!(cc.test(b'a'));
        assert!(cc.test(b'b'));
        assert!(cc.test(b'c'));
        assert!(!cc.test(b'd'));
    }

    #[test]
    fn add_range_swaps_inverted_input() {
        let mut cc = CharClass::new();
        cc.add_range(b'c', b'a');
        assert!(cc.test(b'a'));
        assert!(cc.test(b'b'));
        assert!(cc.test(b'c'));
    }

    #[test]
    fn negate_inverts_membership() {
        let mut cc = CharClass::new();
        cc.add(b'A');
        cc.negate = true;
        assert!(!cc.test(b'A'));
        assert!(cc.test(b'B'));
    }

    #[test]
    fn predefined_digit_class() {
        let mut cc = CharClass::new();
        cc.add_digit();
        for c in b'0'..=b'9' {
            assert!(cc.test(c));
        }
        assert!(!cc.test(b'A'));
    }

    #[test]
    fn predefined_word_class() {
        let mut cc = CharClass::new();
        cc.add_word();
        assert!(cc.test(b'_'));
        for c in b'a'..=b'z' {
            assert!(cc.test(c));
        }
        for c in b'A'..=b'Z' {
            assert!(cc.test(c));
        }
        for c in b'0'..=b'9' {
            assert!(cc.test(c));
        }
        assert!(!cc.test(b' '));
    }

    #[test]
    fn predefined_space_class() {
        let mut cc = CharClass::new();
        cc.add_space();
        for &c in b" \t\n\x0b\x0c\r" {
            assert!(cc.test(c));
        }
        assert!(!cc.test(b'a'));
    }

    #[test]
    fn property_letter_covers_ascii_bitmap_and_ucd_cp() {
        let mut cc = CharClass::new();
        cc.add_property_letter();
        assert!(cc.test(b'A'));
        assert!(cc.test(b'z'));
        assert!(cc.test_cp(b'A' as i32));
        // cp ≥ 0x80 path — Greek alpha α (0x03B1) is in UCD_LETTER.
        assert!(cc.test_cp(0x03B1));
        // Number doesn't get letter set.
        assert!(!cc.test_cp(b'5' as i32));
    }

    #[test]
    fn property_number_covers_ascii_digits_and_ucd_digits() {
        let mut cc = CharClass::new();
        cc.add_property_number();
        for c in b'0'..=b'9' {
            assert!(cc.test(c));
            assert!(cc.test_cp(c as i32));
        }
        // cp ≥ 0x80 — Arabic-Indic 4 (0x0664) is in UCD_NUMBER.
        assert!(cc.test_cp(0x0664));
        assert!(!cc.test_cp(b'A' as i32));
    }

    #[test]
    fn property_ascii_covers_full_low_128() {
        let mut cc = CharClass::new();
        cc.add_property_ascii();
        for c in 0..=0x7Fu8 {
            assert!(cc.test(c));
        }
        // u_props not set → cp ≥ 128 misses.
        assert!(!cc.test_cp(0x0080));
        assert!(!cc.test_cp(0x4E2D));
    }

    #[test]
    fn test_cp_low_path_matches_bits() {
        let mut cc = CharClass::new();
        cc.add(b'X');
        assert!(cc.test_cp(b'X' as i32));
        assert!(!cc.test_cp(b'Y' as i32));
    }

    #[test]
    fn test_cp_negate_applies_after_union() {
        let mut cc = CharClass::new();
        cc.add_property_letter();
        cc.negate = true;
        // Letter cp is now NOT a member.
        assert!(!cc.test_cp(b'A' as i32));
        assert!(!cc.test_cp(0x03B1)); // Greek alpha
        // Non-letter cp now IS a member.
        assert!(cc.test_cp(b'5' as i32));
    }

    #[test]
    fn clear_resets_state() {
        let mut cc = CharClass::new();
        cc.add(b'A');
        cc.add_property_letter();
        cc.negate = true;
        cc.clear();
        assert!(!cc.test(b'A'));
        assert!(!cc.negate);
        assert_eq!(cc.u_props, 0);
    }
}
