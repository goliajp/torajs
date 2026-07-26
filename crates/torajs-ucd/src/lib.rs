//! Curated Unicode Character Database subset for ECMAScript regex
//! `\p{NAME}` property classes.
//!
//! Extracted from the torajs AOT TypeScript runtime
//! (the pre-rewrite `runtime_regex.c` UCD section,
//! P9.3-A2 ship 2026-05-19). Provides binary-searchable static tables
//! covering the dominant test262 usage subset of Letter (`L`) and
//! Number (`N`) categories.
//!
//! ## Scope
//!
//! `UCD_LETTER` / `UCD_NUMBER` are an **intentional partial cover**, not
//! a full UCD. Picked to lift the dominant test262 cases at minimum
//! code-size cost. (`ID_START` / `ID_CONTINUE`, added later for the
//! lexer, are the opposite: generated from `DerivedCoreProperties.txt`
//! and complete.) Coverage:
//!
//! - **Letter (L)** — Latin-1 supplement, IPA + Spacing Modifier,
//!   Greek + Coptic, Cyrillic, Armenian, Hebrew, Arabic, Devanagari,
//!   Thai, Hiragana, Katakana, CJK Unified Ideographs (basic + ext A),
//!   Hangul Syllables.
//! - **Number (N)** — Latin-1 numeric, Arabic-Indic digits, NKo,
//!   Devanagari/Bengali/Gurmukhi/Gujarati/Oriya/Tamil/Telugu/Kannada/
//!   Malayalam/Sinhala/Thai/Lao/Tibetan/Myanmar/Khmer/Mongolian digits,
//!   Fullwidth digits.
//!
//! ASCII portion (cp < 128) is **not** included here — callers are
//! expected to bitmap-test ASCII separately (matches the regex-VM
//! convention of testing the per-class 32-byte bitmap first, then
//! falling through to the cp ≥ 128 property tables).
//!
//! Full-coverage UCD import (auto-generated from `UnicodeData.txt`) is
//! tracked as an L3b follow-up in the parent torajs project.
//!
//! ## API
//!
//! ```
//! use torajs_ucd::{is_letter_cp, is_number_cp};
//!
//! assert!(is_letter_cp(0x4E2D));   // 中 (CJK)
//! assert!(is_letter_cp(0x03B1));   // α (Greek lowercase alpha)
//! assert!(!is_letter_cp(0x0030));  // '0' is ASCII, callers bitmap-test
//! assert!(!is_letter_cp(0x2022));  // bullet, not a letter
//!
//! assert!(is_number_cp(0x0660));   // Arabic-Indic 0
//! assert!(is_number_cp(0xFF15));   // Fullwidth 5
//! assert!(!is_number_cp(0x0030));  // '0' is ASCII, callers bitmap-test
//! ```
//!
//! ## Performance
//!
//! Each lookup is a single binary search over the static range table —
//! O(log N) where N is the per-property range count (~50 for L, ~30 for
//! N as of v0.1.0). On a modern aarch64 the cost is < 20 ns / lookup
//! (see `benches/ucd.rs`).

#![no_std]

mod id_table;

pub use id_table::{ID_CONTINUE, ID_START};

/// A `(lo, hi)` inclusive codepoint range.
pub type Range = (u32, u32);

/// Curated Letter (L) ranges. cp ≥ 128 portion of Unicode Letter
/// category as covered by torajs's subset. ASCII letters live in the
/// caller's bitmap.
pub static UCD_LETTER: &[Range] = &[
    // Latin-1 supplement letters (cp > 0x7F)
    (0x00AA, 0x00AA),
    (0x00B5, 0x00B5),
    (0x00BA, 0x00BA),
    (0x00C0, 0x00D6),
    (0x00D8, 0x00F6),
    (0x00F8, 0x024F),
    // IPA + Spacing Modifier
    (0x0250, 0x02AF),
    (0x02B0, 0x02C1),
    (0x02C6, 0x02D1),
    (0x02E0, 0x02E4),
    (0x02EC, 0x02EC),
    (0x02EE, 0x02EE),
    // Greek and Coptic
    (0x0370, 0x0373),
    (0x0376, 0x0377),
    (0x037A, 0x037D),
    (0x037F, 0x037F),
    (0x0386, 0x0386),
    (0x0388, 0x038A),
    (0x038C, 0x038C),
    (0x038E, 0x03A1),
    (0x03A3, 0x03F5),
    (0x03F7, 0x0481),
    // Cyrillic
    (0x048A, 0x052F),
    // Armenian
    (0x0531, 0x0556),
    (0x0561, 0x0587),
    // Hebrew letters
    (0x05D0, 0x05EA),
    (0x05F0, 0x05F2),
    // Arabic letters
    (0x0620, 0x064A),
    (0x066E, 0x066F),
    (0x0671, 0x06D3),
    (0x06D5, 0x06D5),
    (0x06E5, 0x06E6),
    (0x06EE, 0x06EF),
    (0x06FA, 0x06FC),
    (0x06FF, 0x06FF),
    // Devanagari letters
    (0x0904, 0x0939),
    (0x093D, 0x093D),
    (0x0950, 0x0950),
    (0x0958, 0x0961),
    // Thai letters
    (0x0E01, 0x0E30),
    (0x0E32, 0x0E33),
    (0x0E40, 0x0E46),
    // Hiragana
    (0x3041, 0x3096),
    (0x309D, 0x309F),
    // Katakana
    (0x30A1, 0x30FA),
    (0x30FC, 0x30FF),
    // CJK Unified Ideographs (basic + extension A)
    (0x3400, 0x4DBF),
    (0x4E00, 0x9FFF),
    // Hangul Syllables
    (0xAC00, 0xD7A3),
];

/// Curated Number (N) ranges. cp ≥ 128 portion of Unicode Number
/// category as covered by torajs's subset. ASCII digits live in the
/// caller's bitmap.
pub static UCD_NUMBER: &[Range] = &[
    // Latin-1 numeric
    (0x00B2, 0x00B3),
    (0x00B9, 0x00B9),
    (0x00BC, 0x00BE),
    // Arabic-Indic digits
    (0x0660, 0x0669),
    (0x06F0, 0x06F9),
    // NKo
    (0x07C0, 0x07C9),
    // Devanagari digits
    (0x0966, 0x096F),
    // Bengali
    (0x09E6, 0x09EF),
    (0x09F4, 0x09F9),
    // Gurmukhi / Gujarati / Oriya / Tamil / Telugu / Kannada / Malayalam
    (0x0A66, 0x0A6F),
    (0x0AE6, 0x0AEF),
    (0x0B66, 0x0B6F),
    (0x0BE6, 0x0BF2),
    (0x0C66, 0x0C6F),
    (0x0CE6, 0x0CEF),
    (0x0D66, 0x0D75),
    // Sinhala / Thai / Lao / Tibetan / Myanmar
    (0x0DE6, 0x0DEF),
    (0x0E50, 0x0E59),
    (0x0ED0, 0x0ED9),
    (0x0F20, 0x0F33),
    (0x1040, 0x1049),
    (0x1090, 0x1099),
    // Khmer / Mongolian
    (0x17E0, 0x17E9),
    (0x1810, 0x1819),
    // Fullwidth digits
    (0xFF10, 0xFF19),
];

/// Binary-search a sorted, non-overlapping range table for membership.
/// `O(log N)`. Returns true iff `cp` is in any `(lo, hi)` range.
///
/// Caller's responsibility to maintain the sorted+non-overlapping
/// invariant when constructing custom range tables. The shipped
/// `UCD_LETTER` and `UCD_NUMBER` tables are pre-sorted.
#[inline]
pub fn range_contains(table: &[Range], cp: u32) -> bool {
    let mut lo = 0usize;
    let mut hi = table.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let (rlo, rhi) = table[mid];
        if cp < rlo {
            hi = mid;
        } else if cp > rhi {
            lo = mid + 1;
        } else {
            return true;
        }
    }
    false
}

/// Test whether `cp` is in the curated Unicode Letter category (cp ≥ 128
/// portion). Returns false for ASCII letters — caller bitmap-tests those.
#[inline]
pub fn is_letter_cp(cp: u32) -> bool {
    range_contains(UCD_LETTER, cp)
}

/// Test whether `cp` is in the curated Unicode Number category (cp ≥ 128
/// portion). Returns false for ASCII digits — caller bitmap-tests those.
#[inline]
pub fn is_number_cp(cp: u32) -> bool {
    range_contains(UCD_NUMBER, cp)
}

/// Test whether `cp` can start an identifier — Unicode `ID_Start`, which
/// is what ES §12.7.1 `UnicodeIDStart` names (cp ≥ 128 portion). Returns
/// false for ASCII, including the `$` and `_` that §12.7.1 also admits:
/// the caller bitmap-tests those, same as `is_letter_cp`.
///
/// Unlike `UCD_LETTER` this is **full coverage**, not a curated subset —
/// `ID_START` is generated from `DerivedCoreProperties.txt`. It is also
/// not the same set as Letter: `ID_Start` picks up Nl and Other_ID_Start
/// (U+2118 ℘ and U+212E ℮ are the ones test262 exercises) while dropping
/// Pattern_Syntax characters.
#[inline]
pub fn is_id_start_cp(cp: u32) -> bool {
    range_contains(id_table::ID_START, cp)
}

/// Test whether `cp` can continue an identifier — Unicode `ID_Continue`
/// plus U+200C ZWNJ and U+200D ZWJ, which ES §12.7.1 names separately in
/// `IdentifierPartChar` (cp ≥ 128 portion; ASCII is the caller's).
#[inline]
pub fn is_id_continue_cp(cp: u32) -> bool {
    range_contains(id_table::ID_CONTINUE, cp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_start_letters_and_other_id_start() {
        assert!(is_id_start_cp(0x4E2D)); // 中
        assert!(is_id_start_cp(0x03B1)); // α
        // Other_ID_Start — not Letter category, but identifier-startable.
        // test262's class-elements privatename tests use exactly these.
        assert!(is_id_start_cp(0x2118)); // ℘ SCRIPT CAPITAL P (Sm)
        assert!(is_id_start_cp(0x212E)); // ℮ ESTIMATED SIGN (So)
        assert!(!is_letter_cp(0x2118)); // the reason ID_Start != Letter
    }

    #[test]
    fn id_start_rejects_ascii_and_punctuation() {
        // ASCII is the caller's bitmap, including the `$`/`_` that
        // §12.7.1 admits alongside UnicodeIDStart.
        assert!(!is_id_start_cp(b'a' as u32));
        assert!(!is_id_start_cp(b'$' as u32));
        assert!(!is_id_start_cp(b'_' as u32));
        assert!(!is_id_start_cp(0x2022)); // bullet
        assert!(!is_id_start_cp(0x00A0)); // NBSP
    }

    #[test]
    fn id_continue_adds_joiners_and_marks() {
        // ZWNJ/ZWJ are named by ES, not carried by the Unicode property.
        assert!(is_id_continue_cp(0x200C));
        assert!(is_id_continue_cp(0x200D));
        assert!(!is_id_start_cp(0x200C));
        assert!(!is_id_start_cp(0x200D));
        assert!(is_id_continue_cp(0x0301)); // combining acute (Mn)
        assert!(is_id_continue_cp(0x0660)); // Arabic-Indic zero (Nd)
        assert!(is_id_continue_cp(0x4E2D)); // ID_Start ⊂ ID_Continue
        assert!(!is_id_continue_cp(0x2022)); // bullet
    }

    #[test]
    fn id_tables_are_sorted_and_disjoint() {
        // range_contains binary-searches; the invariant is load-bearing.
        for table in [ID_START, ID_CONTINUE] {
            for w in table.windows(2) {
                assert!(w[0].0 <= w[0].1);
                assert!(w[0].1 < w[1].0, "overlap at {:#X}", w[0].0);
            }
        }
    }

    #[test]
    fn id_start_is_a_subset_of_id_continue() {
        // UAX #31: ID_Continue = ID_Start + Mn/Mc/Nd/Pc + Other_ID_Continue.
        // The lexer leans on this — it walks the *whole* identifier,
        // first codepoint included, with the ID_Continue predicate.
        for &(lo, hi) in ID_START {
            assert!(is_id_continue_cp(lo), "{lo:#X} starts but cannot continue");
            assert!(is_id_continue_cp(hi), "{hi:#X} starts but cannot continue");
        }
    }

    #[test]
    fn letter_cjk_basic() {
        assert!(is_letter_cp(0x4E2D)); // 中
        assert!(is_letter_cp(0x65E5)); // 日
        assert!(is_letter_cp(0x672C)); // 本
        assert!(is_letter_cp(0x3400)); // CJK Ext-A start
        assert!(is_letter_cp(0x9FFF)); // CJK basic end
    }

    #[test]
    fn letter_hiragana_katakana() {
        assert!(is_letter_cp(0x3042)); // あ
        assert!(is_letter_cp(0x30A2)); // ア
    }

    #[test]
    fn letter_greek() {
        assert!(is_letter_cp(0x03B1)); // α
        assert!(is_letter_cp(0x03A9)); // Ω
    }

    #[test]
    fn letter_hebrew() {
        assert!(is_letter_cp(0x05D0)); // א
        assert!(is_letter_cp(0x05EA)); // ת
    }

    #[test]
    fn letter_arabic() {
        assert!(is_letter_cp(0x0627)); // ا
        assert!(is_letter_cp(0x064A)); // ي
    }

    #[test]
    fn letter_hangul() {
        assert!(is_letter_cp(0xAC00)); // 가
        assert!(is_letter_cp(0xD7A3)); // 힣
    }

    #[test]
    fn letter_ascii_excluded() {
        assert!(!is_letter_cp(b'a' as u32));
        assert!(!is_letter_cp(b'Z' as u32));
        assert!(!is_letter_cp(b'0' as u32));
        assert!(!is_letter_cp(0x007F)); // DEL
    }

    #[test]
    fn letter_non_letter_punctuation() {
        assert!(!is_letter_cp(0x2022)); // bullet
        assert!(!is_letter_cp(0x2026)); // ellipsis
        assert!(!is_letter_cp(0x3001)); // 、 (CJK comma)
    }

    #[test]
    fn number_arabic_indic() {
        assert!(is_number_cp(0x0660)); // ٠
        assert!(is_number_cp(0x0669)); // ٩
    }

    #[test]
    fn number_devanagari() {
        assert!(is_number_cp(0x0966)); // ०
        assert!(is_number_cp(0x096F)); // ९
    }

    #[test]
    fn number_fullwidth() {
        assert!(is_number_cp(0xFF10)); // 0 (full-width)
        assert!(is_number_cp(0xFF15)); // 5 (full-width)
        assert!(is_number_cp(0xFF19)); // 9 (full-width)
    }

    #[test]
    fn number_ascii_excluded() {
        assert!(!is_number_cp(b'0' as u32));
        assert!(!is_number_cp(b'9' as u32));
    }

    #[test]
    fn range_contains_empty_table() {
        let empty: &[Range] = &[];
        assert!(!range_contains(empty, 0x4E2D));
    }

    #[test]
    fn range_contains_boundary() {
        let table: &[Range] = &[(0x10, 0x20), (0x40, 0x50)];
        assert!(range_contains(table, 0x10)); // lo inclusive
        assert!(range_contains(table, 0x20)); // hi inclusive
        assert!(range_contains(table, 0x18)); // middle
        assert!(!range_contains(table, 0x21));
        assert!(!range_contains(table, 0x30)); // gap
        assert!(range_contains(table, 0x50));
    }

    #[test]
    fn table_sorted_invariant() {
        // Sanity check that the shipped tables are sorted +
        // non-overlapping — otherwise the binary search returns
        // wrong answers.
        for table in [UCD_LETTER, UCD_NUMBER] {
            let mut prev_hi: i64 = -1;
            for &(lo, hi) in table {
                assert!(
                    (lo as i64) > prev_hi,
                    "table not sorted / overlapping: lo={lo:#x} prev_hi={prev_hi:#x}"
                );
                assert!(hi >= lo, "range hi < lo: ({lo:#x}, {hi:#x})");
                prev_hi = hi as i64;
            }
        }
    }
}
