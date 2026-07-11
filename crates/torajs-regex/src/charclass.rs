//! Char class — port of `runtime_regex.c` L254-349.
//!
//! 256-bit ASCII bitmap + inversion bit + Unicode property table
//! references. One per `OP_CLASS` instruction in the future Program
//! (interned by `Program.classes[]` in P6.2-c).
//!
//! ASCII portion lives in [`CharClass::bits`]; under the u flag, the
//! cp ≥ 128 portion is covered by the CODEGEN UCD tables referenced
//! from [`CharClass::u_prop_tables`] and consulted via
//! [`CharClass::test_cp`].

use crate::ucd::{UPropRange, uprop_range_contains};
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharClass {
    pub bits: [u8; 32],
    pub negate: bool,
    /// Unicode property range tables (RFC 20260711 chunk B). Each
    /// `\p{...}` in a u-flag pattern resolves to one `'static` CODEGEN
    /// table reference pushed here via
    /// [`add_property_table`](Self::add_property_table);
    /// [`test_cp`](Self::test_cp) binary-searches every referenced
    /// table for `cp ≥ 128`. The ASCII portion of each property lives
    /// in `bits` (populated at push time). Class-level `negate` still
    /// applies after the union.
    pub u_prop_tables: Vec<&'static [UPropRange]>,
    /// chunk 10d marker — `true` for the byte-level leaf classes
    /// emitted by [`crate::utf8_class_expand`] when rewriting a u-flag
    /// unsafe class into an Alt-of-Concat of byte slots. The Pike VM
    /// `Op::Class` interpreter honours this by stepping a single byte
    /// (no UTF-8 cp decode) regardless of `RE_FLAG_U`, so the second-
    /// pass capture-recovery walk lines up with the DFA's byte-step
    /// fast path. Default `false` keeps all hand-written classes on
    /// the historical cp-aware path.
    pub byte_only: bool,
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
            u_prop_tables: Vec::new(),
            byte_only: false,
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

    /// Round 3 Path A — true iff this class is a "pure property" u-flag
    /// unsafe class: property tables referenced, no `negate`, no
    /// explicit non-ASCII byte bits (`bits[16..32]` all zero).
    /// K-PROPERTY classes skip chunk-10d Alt-of-Concat expansion; the
    /// DFA build pre-emits a pending K-PROPERTY state whose executor
    /// handler decodes one UTF-8 cp per step and consults
    /// [`Self::test_cp`]. Table-free unsafe kinds (K-NEG
    /// `negate == true`, K-EXPLICIT-BIT non-ASCII bits set) still
    /// flow through `utf8_class_expand` and the chunk-10d byte-step
    /// path; table-bearing negated / mixed classes decline expansion
    /// (RFC 20260711 chunk B) and are Pike-VM-served —
    /// `prog_ops_dfa_safe` gates their programs off the DFA.
    pub fn is_uflag_property_only(&self) -> bool {
        !self.u_prop_tables.is_empty() && !self.negate && self.bits[16..32].iter().all(|&b| b == 0)
    }

    /// Code-point membership test for the u flag — port of `cc_test_cp`.
    ///
    /// `cp < 128` is bitmap-tested. `cp ≥ 128` with no property tables
    /// referenced is a miss (bitmap doesn't reach there). `cp ≥ 128`
    /// with tables referenced binary-searches each. Class-level
    /// `negate` inverts after the union.
    pub fn test_cp(&self, cp: i32) -> bool {
        let mut in_set = false;
        if (0..256).contains(&cp) {
            in_set = (self.bits[(cp >> 3) as usize] >> (cp & 7)) & 1 != 0;
        }
        if !in_set && cp >= 0x80 {
            in_set = self
                .u_prop_tables
                .iter()
                .any(|t| uprop_range_contains(t, cp));
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

    /// `\p{...}` — union a resolved UCD property range table into the
    /// class. The `cp < 128` portion is baked into `bits` (so the DFA
    /// ASCII byte path and `test` keep working); the table reference
    /// is recorded only when it reaches past ASCII — an ASCII-only
    /// property (`\p{ASCII}`, `\p{AHex}`, …) stays a pure-bitmap
    /// class and keeps the u-safe DFA fast path.
    pub fn add_property_table(&mut self, table: &'static [UPropRange]) {
        let mut reaches_high = false;
        for r in table {
            if r.lo <= 0x7F {
                for cp in r.lo..=r.hi.min(0x7F) {
                    self.add(cp as u8);
                }
            }
            if r.hi > 0x7F {
                reaches_high = true;
            }
        }
        if reaches_high {
            self.u_prop_tables.push(table);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ucd::{lookup_binary, lookup_gc_value};

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
        cc.add_property_table(lookup_gc_value("L").unwrap());
        assert!(cc.test(b'A'));
        assert!(cc.test(b'z'));
        assert!(cc.test_cp(b'A' as i32));
        // cp ≥ 0x80 path — Greek alpha α (0x03B1) is in gc L.
        assert!(cc.test_cp(0x03B1));
        // Number doesn't get letter set.
        assert!(!cc.test_cp(b'5' as i32));
    }

    #[test]
    fn property_number_covers_ascii_digits_and_ucd_digits() {
        let mut cc = CharClass::new();
        cc.add_property_table(lookup_gc_value("N").unwrap());
        for c in b'0'..=b'9' {
            assert!(cc.test(c));
            assert!(cc.test_cp(c as i32));
        }
        // cp ≥ 0x80 — Arabic-Indic 4 (0x0664) is in gc N.
        assert!(cc.test_cp(0x0664));
        assert!(!cc.test_cp(b'A' as i32));
    }

    #[test]
    fn property_ascii_stays_pure_bitmap() {
        let mut cc = CharClass::new();
        cc.add_property_table(lookup_binary("ASCII").unwrap());
        for c in 0..=0x7Fu8 {
            assert!(cc.test(c));
        }
        // ASCII-only table is NOT recorded → cp ≥ 128 misses and the
        // class keeps the u-safe pure-bitmap DFA path.
        assert!(cc.u_prop_tables.is_empty());
        assert!(!cc.is_uflag_property_only());
        assert!(!cc.test_cp(0x0080));
        assert!(!cc.test_cp(0x4E2D));
    }

    #[test]
    fn property_union_of_two_tables() {
        // `[\p{L}\p{N}]` shape — both tables consulted.
        let mut cc = CharClass::new();
        cc.add_property_table(lookup_gc_value("L").unwrap());
        cc.add_property_table(lookup_gc_value("N").unwrap());
        assert!(cc.test_cp(0x03B1)); // α — letter
        assert!(cc.test_cp(0x0664)); // ٤ — number
        assert!(!cc.test_cp(0x0021)); // '!' — neither
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
        cc.add_property_table(lookup_gc_value("L").unwrap());
        cc.negate = true;
        // Letter cp is now NOT a member.
        assert!(!cc.test_cp(b'A' as i32));
        assert!(!cc.test_cp(0x03B1)); // Greek alpha
        // Non-letter cp now IS a member.
        assert!(cc.test_cp(b'5' as i32));
    }

    #[test]
    fn is_uflag_property_only_classifies_correctly() {
        // Round 3 Path A v2 — predicate gate for the DFA pending K-PROPERTY
        // emit. Must return true only for "pure property" classes (tables
        // referenced, no negate, no explicit non-ASCII bits); K-NEG /
        // K-MIXED / K-EXPLICIT-BIT must return false so they keep flowing
        // through chunk-10d utf8_class_expand.
        let mut letter = CharClass::new();
        letter.add_property_table(lookup_gc_value("L").unwrap());
        assert!(
            letter.is_uflag_property_only(),
            "pure \\p{{L}} = K-PROPERTY"
        );

        let mut letter_neg = letter.clone();
        letter_neg.negate = true;
        assert!(
            !letter_neg.is_uflag_property_only(),
            "negated \\p{{L}} is K-NEG"
        );

        let mut digit_safe = CharClass::new();
        digit_safe.add_digit();
        assert!(
            !digit_safe.is_uflag_property_only(),
            "\\d has no property tables"
        );

        let mut letter_plus_explicit = letter.clone();
        // Set a bit in bits[16..32] (non-ASCII byte range) — K-MIXED.
        letter_plus_explicit.bits[24] = 0x01;
        assert!(
            !letter_plus_explicit.is_uflag_property_only(),
            "\\p{{L}} + explicit non-ASCII byte bit = K-MIXED"
        );
    }

    #[test]
    fn clear_resets_state() {
        let mut cc = CharClass::new();
        cc.add(b'A');
        cc.add_property_table(lookup_gc_value("L").unwrap());
        cc.negate = true;
        cc.byte_only = true;
        cc.clear();
        assert!(!cc.test(b'A'));
        assert!(!cc.negate);
        assert!(cc.u_prop_tables.is_empty());
        assert!(!cc.byte_only);
    }
}
