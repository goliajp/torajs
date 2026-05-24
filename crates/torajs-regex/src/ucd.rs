//! Curated Unicode property tables — port of `runtime_regex.c`
//! L155-251.
//!
//! Subsets of UCD Letter / Number categories covering the dominant
//! test262 usages (Greek, Cyrillic, Hebrew, Arabic, CJK, Hangul,
//! Hiragana, Katakana, common decimal-digit scripts).
//!
//! ASCII portions live in the [`super::charclass::CharClass`]
//! bitmap (populated by `add_property_*`). Code points ≥ 128 are
//! covered by these range tables, scanned via binary search by
//! [`uprop_range_contains`].
//!
//! The full UCD Letter category has hundreds of ranges; the curated
//! subset here is intentionally a partial cover — per
//! `docs/design-principles.md`'s "正统 / textbook" pragma, this is
//! minimum-viable: lift the dominant test262 cases, then iterate.
//! L3b follow-up: full UCD import or generated table.

/// `CharClass::u_props` bit — `\p{L}` (Letter).
pub const UP_LETTER: u8 = 0x01;

/// `CharClass::u_props` bit — `\p{N}` (Number).
pub const UP_NUMBER: u8 = 0x02;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UPropRange {
    pub lo: i32,
    pub hi: i32,
}

pub const UCD_LETTER: &[UPropRange] = &[
    // Latin-1 supplement letters (cp > 0x7F)
    UPropRange {
        lo: 0x00AA,
        hi: 0x00AA,
    },
    UPropRange {
        lo: 0x00B5,
        hi: 0x00B5,
    },
    UPropRange {
        lo: 0x00BA,
        hi: 0x00BA,
    },
    UPropRange {
        lo: 0x00C0,
        hi: 0x00D6,
    },
    UPropRange {
        lo: 0x00D8,
        hi: 0x00F6,
    },
    UPropRange {
        lo: 0x00F8,
        hi: 0x024F,
    },
    // IPA + Spacing Modifier
    UPropRange {
        lo: 0x0250,
        hi: 0x02AF,
    },
    UPropRange {
        lo: 0x02B0,
        hi: 0x02C1,
    },
    UPropRange {
        lo: 0x02C6,
        hi: 0x02D1,
    },
    UPropRange {
        lo: 0x02E0,
        hi: 0x02E4,
    },
    UPropRange {
        lo: 0x02EC,
        hi: 0x02EC,
    },
    UPropRange {
        lo: 0x02EE,
        hi: 0x02EE,
    },
    // Greek and Coptic
    UPropRange {
        lo: 0x0370,
        hi: 0x0373,
    },
    UPropRange {
        lo: 0x0376,
        hi: 0x0377,
    },
    UPropRange {
        lo: 0x037A,
        hi: 0x037D,
    },
    UPropRange {
        lo: 0x037F,
        hi: 0x037F,
    },
    UPropRange {
        lo: 0x0386,
        hi: 0x0386,
    },
    UPropRange {
        lo: 0x0388,
        hi: 0x038A,
    },
    UPropRange {
        lo: 0x038C,
        hi: 0x038C,
    },
    UPropRange {
        lo: 0x038E,
        hi: 0x03A1,
    },
    UPropRange {
        lo: 0x03A3,
        hi: 0x03F5,
    },
    UPropRange {
        lo: 0x03F7,
        hi: 0x0481,
    },
    // Cyrillic
    UPropRange {
        lo: 0x048A,
        hi: 0x052F,
    },
    // Armenian
    UPropRange {
        lo: 0x0531,
        hi: 0x0556,
    },
    UPropRange {
        lo: 0x0561,
        hi: 0x0587,
    },
    // Hebrew letters
    UPropRange {
        lo: 0x05D0,
        hi: 0x05EA,
    },
    UPropRange {
        lo: 0x05F0,
        hi: 0x05F2,
    },
    // Arabic letters
    UPropRange {
        lo: 0x0620,
        hi: 0x064A,
    },
    UPropRange {
        lo: 0x066E,
        hi: 0x066F,
    },
    UPropRange {
        lo: 0x0671,
        hi: 0x06D3,
    },
    UPropRange {
        lo: 0x06D5,
        hi: 0x06D5,
    },
    UPropRange {
        lo: 0x06E5,
        hi: 0x06E6,
    },
    UPropRange {
        lo: 0x06EE,
        hi: 0x06EF,
    },
    UPropRange {
        lo: 0x06FA,
        hi: 0x06FC,
    },
    UPropRange {
        lo: 0x06FF,
        hi: 0x06FF,
    },
    // Devanagari letters
    UPropRange {
        lo: 0x0904,
        hi: 0x0939,
    },
    UPropRange {
        lo: 0x093D,
        hi: 0x093D,
    },
    UPropRange {
        lo: 0x0950,
        hi: 0x0950,
    },
    UPropRange {
        lo: 0x0958,
        hi: 0x0961,
    },
    // Thai letters
    UPropRange {
        lo: 0x0E01,
        hi: 0x0E30,
    },
    UPropRange {
        lo: 0x0E32,
        hi: 0x0E33,
    },
    UPropRange {
        lo: 0x0E40,
        hi: 0x0E46,
    },
    // Hiragana
    UPropRange {
        lo: 0x3041,
        hi: 0x3096,
    },
    UPropRange {
        lo: 0x309D,
        hi: 0x309F,
    },
    // Katakana
    UPropRange {
        lo: 0x30A1,
        hi: 0x30FA,
    },
    UPropRange {
        lo: 0x30FC,
        hi: 0x30FF,
    },
    // CJK Unified Ideographs (basic + extension A)
    UPropRange {
        lo: 0x3400,
        hi: 0x4DBF,
    },
    UPropRange {
        lo: 0x4E00,
        hi: 0x9FFF,
    },
    // Hangul Syllables
    UPropRange {
        lo: 0xAC00,
        hi: 0xD7A3,
    },
];

pub const UCD_NUMBER: &[UPropRange] = &[
    // Latin-1 numeric
    UPropRange {
        lo: 0x00B2,
        hi: 0x00B3,
    },
    UPropRange {
        lo: 0x00B9,
        hi: 0x00B9,
    },
    UPropRange {
        lo: 0x00BC,
        hi: 0x00BE,
    },
    // Arabic-Indic digits
    UPropRange {
        lo: 0x0660,
        hi: 0x0669,
    },
    UPropRange {
        lo: 0x06F0,
        hi: 0x06F9,
    },
    // NKo
    UPropRange {
        lo: 0x07C0,
        hi: 0x07C9,
    },
    // Devanagari digits
    UPropRange {
        lo: 0x0966,
        hi: 0x096F,
    },
    // Bengali
    UPropRange {
        lo: 0x09E6,
        hi: 0x09EF,
    },
    UPropRange {
        lo: 0x09F4,
        hi: 0x09F9,
    },
    // Gurmukhi / Gujarati / Oriya / Tamil / Telugu / Kannada / Malayalam
    UPropRange {
        lo: 0x0A66,
        hi: 0x0A6F,
    },
    UPropRange {
        lo: 0x0AE6,
        hi: 0x0AEF,
    },
    UPropRange {
        lo: 0x0B66,
        hi: 0x0B6F,
    },
    UPropRange {
        lo: 0x0BE6,
        hi: 0x0BF2,
    },
    UPropRange {
        lo: 0x0C66,
        hi: 0x0C6F,
    },
    UPropRange {
        lo: 0x0CE6,
        hi: 0x0CEF,
    },
    UPropRange {
        lo: 0x0D66,
        hi: 0x0D75,
    },
    // Sinhala / Thai / Lao / Tibetan / Myanmar
    UPropRange {
        lo: 0x0DE6,
        hi: 0x0DEF,
    },
    UPropRange {
        lo: 0x0E50,
        hi: 0x0E59,
    },
    UPropRange {
        lo: 0x0ED0,
        hi: 0x0ED9,
    },
    UPropRange {
        lo: 0x0F20,
        hi: 0x0F33,
    },
    UPropRange {
        lo: 0x1040,
        hi: 0x1049,
    },
    UPropRange {
        lo: 0x1090,
        hi: 0x1099,
    },
    // Khmer / Mongolian
    UPropRange {
        lo: 0x17E0,
        hi: 0x17E9,
    },
    UPropRange {
        lo: 0x1810,
        hi: 0x1819,
    },
    // Fullwidth digits
    UPropRange {
        lo: 0xFF10,
        hi: 0xFF19,
    },
];

pub fn uprop_range_contains(t: &[UPropRange], cp: i32) -> bool {
    let mut lo: isize = 0;
    let mut hi: isize = t.len() as isize - 1;
    while lo <= hi {
        let mid = ((lo + hi) >> 1) as usize;
        if cp < t[mid].lo {
            hi = mid as isize - 1;
        } else if cp > t[mid].hi {
            lo = mid as isize + 1;
        } else {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted_and_disjoint() {
        for table in [UCD_LETTER, UCD_NUMBER] {
            for w in table.windows(2) {
                assert!(w[0].hi < w[1].lo, "ranges must be sorted + disjoint");
            }
        }
    }

    #[test]
    fn letter_hits() {
        assert!(uprop_range_contains(UCD_LETTER, 0x03B1)); // α (Greek)
        assert!(uprop_range_contains(UCD_LETTER, 0x0451)); // ё (Cyrillic)
        assert!(uprop_range_contains(UCD_LETTER, 0x4E2D)); // 中
        assert!(uprop_range_contains(UCD_LETTER, 0xAC00)); // 가
        assert!(uprop_range_contains(UCD_LETTER, 0x3042)); // あ
    }

    #[test]
    fn letter_misses() {
        assert!(!uprop_range_contains(UCD_LETTER, b'A' as i32)); // ASCII not in table
        assert!(!uprop_range_contains(UCD_LETTER, 0x0030)); // '0'
        assert!(!uprop_range_contains(UCD_LETTER, 0xD7A4)); // just past Hangul
    }

    #[test]
    fn number_hits_and_misses() {
        assert!(uprop_range_contains(UCD_NUMBER, 0x0664)); // ٤ (Arabic-Indic 4)
        assert!(uprop_range_contains(UCD_NUMBER, 0xFF15)); // ５ (fullwidth)
        assert!(!uprop_range_contains(UCD_NUMBER, b'5' as i32)); // ASCII not in table
        assert!(!uprop_range_contains(UCD_NUMBER, 0x4E2D)); // 中 is letter not number
    }

    #[test]
    fn boundary_inclusive() {
        for r in UCD_LETTER.iter().take(3) {
            assert!(uprop_range_contains(UCD_LETTER, r.lo));
            assert!(uprop_range_contains(UCD_LETTER, r.hi));
        }
    }
}
