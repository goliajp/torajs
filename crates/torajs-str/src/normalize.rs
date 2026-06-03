//! Unicode normalization (UAX #15) — P11.6 substrate.
//!
//! Sub-step status:
//! - S1 (current): generator + embed tables in `norm_table.rs` + smoke
//!   tests that exercise `lookup_decomp` / `lookup_ccc` /
//!   `lookup_composite` / `lookup_qc` against UCD-known cps.
//! - S2: `decompose` + canonical ordering (incl. Hangul algorithmic).
//! - S3: `compose` (canonical + Hangul).
//! - S4: SSA dispatch for `String.prototype.normalize(form?)`.
//! - S5: fixture pack + integration ship.

#![allow(dead_code)]

use alloc::vec::Vec;

/// Hangul algorithmic constants per UAX #15 D118-D119.
pub(crate) const SBASE: u32 = 0xAC00;
pub(crate) const LBASE: u32 = 0x1100;
pub(crate) const VBASE: u32 = 0x1161;
pub(crate) const TBASE: u32 = 0x11A7;
pub(crate) const LCOUNT: u32 = 19;
pub(crate) const VCOUNT: u32 = 21;
pub(crate) const TCOUNT: u32 = 28;
pub(crate) const NCOUNT: u32 = VCOUNT * TCOUNT; // 588
pub(crate) const SCOUNT: u32 = LCOUNT * NCOUNT; // 11172

#[inline]
pub(crate) fn is_hangul_syllable(cp: u32) -> bool {
    SBASE <= cp && cp < SBASE + SCOUNT
}

/// Hangul algorithmic decomposition. Pushes L+V or L+V+T into `out`.
/// Caller must verify `is_hangul_syllable(cp)` first.
pub(crate) fn hangul_decompose(cp: u32, out: &mut Vec<u32>) {
    let s_index = cp - SBASE;
    let l = LBASE + s_index / NCOUNT;
    let v = VBASE + (s_index % NCOUNT) / TCOUNT;
    let t = TBASE + s_index % TCOUNT;
    out.push(l);
    out.push(v);
    if t != TBASE {
        out.push(t);
    }
}

/// Hangul algorithmic composition. Returns `Some(LV)` for L+V or
/// `Some(LVT)` for LV+T. Returns `None` if `a`/`b` are not Hangul jamo.
#[inline]
pub(crate) fn hangul_compose(a: u32, b: u32) -> Option<u32> {
    // L + V -> LV
    if LBASE <= a && a < LBASE + LCOUNT && VBASE <= b && b < VBASE + VCOUNT {
        let l_index = a - LBASE;
        let v_index = b - VBASE;
        return Some(SBASE + (l_index * VCOUNT + v_index) * TCOUNT);
    }
    // LV + T -> LVT (a must be LV, i.e. a in syllable range with T == 0)
    if SBASE <= a
        && a < SBASE + SCOUNT
        && (a - SBASE) % TCOUNT == 0
        && TBASE < b
        && b < TBASE + TCOUNT
    {
        return Some(a + (b - TBASE));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::norm_table::{
        NFC_QC_BITMAP, NFD_QC_BITMAP, NFKC_QC_BITMAP, NFKD_QC_BITMAP, lookup_ccc, lookup_composite,
        lookup_decomp, lookup_qc,
    };
    use alloc::vec;
    use alloc::vec::Vec;

    // ---------------- DECOMP_TABLE lookups ----------------

    #[test]
    fn decomp_canonical_e_acute() {
        // é U+00E9 -> e (U+0065) + COMBINING ACUTE (U+0301); canonical.
        let (cps, is_compat) = lookup_decomp(0x00E9).expect("é must have a decomposition");
        assert_eq!(cps, &[0x0065, 0x0301]);
        assert!(!is_compat);
    }

    #[test]
    fn decomp_compat_fi_ligature() {
        // ﬁ U+FB01 -> f + i; compat (`<compat>` tag in UnicodeData).
        let (cps, is_compat) = lookup_decomp(0xFB01).expect("ﬁ must have a decomposition");
        assert_eq!(cps, &[0x0066, 0x0069]);
        assert!(is_compat);
    }

    #[test]
    fn decomp_hangul_excluded_from_table() {
        // Hangul syllables (U+AC00..D7A3) are handled algorithmically;
        // they must NOT appear in DECOMP_TABLE.
        assert!(lookup_decomp(0xAC00).is_none()); // 가 (GA, LV syllable)
        assert!(lookup_decomp(0xAC01).is_none()); // 각 (GAG, LVT syllable)
        assert!(lookup_decomp(0xD7A3).is_none()); // last syllable
    }

    #[test]
    fn decomp_ascii_passthrough() {
        // Plain ASCII has no decomposition mapping.
        assert!(lookup_decomp(b'a' as u32).is_none());
        assert!(lookup_decomp(b'Z' as u32).is_none());
    }

    // ---------------- CCC_TABLE lookups ----------------

    #[test]
    fn ccc_starter_default_zero() {
        // Letters / ASCII / Hangul jamo are all starters (CCC 0).
        assert_eq!(lookup_ccc(b'a' as u32), 0);
        assert_eq!(lookup_ccc(0x00E9), 0); // é
        assert_eq!(lookup_ccc(0x1100), 0); // ᄀ (Hangul L)
    }

    #[test]
    fn ccc_combining_marks() {
        // U+0301 COMBINING ACUTE = CCC 230 (Above)
        // U+0327 COMBINING CEDILLA = CCC 202 (Below)
        // U+05B0 HEBREW SHEVA = CCC 10
        assert_eq!(lookup_ccc(0x0301), 230);
        assert_eq!(lookup_ccc(0x0327), 202);
        assert_eq!(lookup_ccc(0x05B0), 10);
    }

    // ---------------- COMPOSE_TABLE lookups ----------------

    #[test]
    fn compose_canonical_e_acute() {
        // e (U+0065) + COMBINING ACUTE (U+0301) -> é (U+00E9)
        assert_eq!(lookup_composite(0x0065, 0x0301), Some(0x00E9));
        // A (U+0041) + COMBINING ACUTE -> Á (U+00C1)
        assert_eq!(lookup_composite(0x0041, 0x0301), Some(0x00C1));
    }

    #[test]
    fn compose_no_pair() {
        // Random non-mapped pair returns None.
        assert!(lookup_composite(b'x' as u32, 0x0301).is_none());
        assert!(lookup_composite(b'a' as u32, b'b' as u32).is_none());
    }

    // ---------------- Quick Check bitmaps ----------------

    #[test]
    fn qc_ascii_all_yes_across_forms() {
        // ASCII range (U+0000..007F) is fully already-normalized in all
        // 4 forms -> Y across the board.
        for cp in 0..0x80 {
            assert_eq!(lookup_qc(NFC_QC_BITMAP, cp), 0, "NFC QC at U+{:04X}", cp);
            assert_eq!(lookup_qc(NFD_QC_BITMAP, cp), 0, "NFD QC at U+{:04X}", cp);
            assert_eq!(lookup_qc(NFKC_QC_BITMAP, cp), 0, "NFKC QC at U+{:04X}", cp);
            assert_eq!(lookup_qc(NFKD_QC_BITMAP, cp), 0, "NFKD QC at U+{:04X}", cp);
        }
    }

    #[test]
    fn qc_nfc_no_for_combining_acute() {
        // U+0301 COMBINING ACUTE: in NFC_QC it is M (Maybe) — it can
        // appear alone, but may combine with a preceding starter.
        // In NFD_QC it is Y (Yes) — combining marks stay decomposed.
        let qc_nfc = lookup_qc(NFC_QC_BITMAP, 0x0301);
        assert!(qc_nfc == 2, "NFC_QC(U+0301) expected M, got {}", qc_nfc);
        assert_eq!(lookup_qc(NFD_QC_BITMAP, 0x0301), 0);
    }

    #[test]
    fn qc_nfd_no_for_e_acute_precomposed() {
        // U+00E9 é precomposed: in NFD_QC it is N (No, must decompose).
        // In NFC_QC it is Y (Yes, already composed).
        assert_eq!(lookup_qc(NFD_QC_BITMAP, 0x00E9), 1);
        assert_eq!(lookup_qc(NFC_QC_BITMAP, 0x00E9), 0);
    }

    #[test]
    fn qc_nfkc_micro_sign_no() {
        // U+00B5 MICRO SIGN has NFKC mapping to U+03BC GREEK SMALL
        // LETTER MU -> NFKC_QC = N, NFKD_QC = N.
        assert_eq!(lookup_qc(NFKC_QC_BITMAP, 0x00B5), 1);
        assert_eq!(lookup_qc(NFKD_QC_BITMAP, 0x00B5), 1);
        // But in canonical forms (NFC/NFD) micro sign stays itself -> Y.
        assert_eq!(lookup_qc(NFC_QC_BITMAP, 0x00B5), 0);
        assert_eq!(lookup_qc(NFD_QC_BITMAP, 0x00B5), 0);
    }

    #[test]
    fn qc_beyond_bitmap_defaults_to_yes() {
        // cp far beyond the bitmap range -> default Y (already-normalized).
        let huge = 0x10_FFFF; // max valid Unicode cp
        assert_eq!(lookup_qc(NFC_QC_BITMAP, huge), 0);
        assert_eq!(lookup_qc(NFD_QC_BITMAP, huge), 0);
    }

    // ---------------- Hangul algorithmic ----------------

    #[test]
    fn hangul_decompose_lv() {
        // 가 U+AC00 (GA) -> ᄀ U+1100 + ᅡ U+1161 (no T)
        let mut out = Vec::new();
        hangul_decompose(0xAC00, &mut out);
        assert_eq!(out, vec![0x1100, 0x1161]);
    }

    #[test]
    fn hangul_decompose_lvt() {
        // 각 U+AC01 (GAG) -> ᄀ + ᅡ + ᆨ (T = U+11A8)
        let mut out = Vec::new();
        hangul_decompose(0xAC01, &mut out);
        assert_eq!(out, vec![0x1100, 0x1161, 0x11A8]);
    }

    #[test]
    fn hangul_compose_l_v() {
        // ᄀ + ᅡ -> 가
        assert_eq!(hangul_compose(0x1100, 0x1161), Some(0xAC00));
    }

    #[test]
    fn hangul_compose_lv_t() {
        // 가 (LV) + ᆨ (T = U+11A8) -> 각 U+AC01
        assert_eq!(hangul_compose(0xAC00, 0x11A8), Some(0xAC01));
    }

    #[test]
    fn hangul_compose_no_match() {
        // Non-Hangul or invalid pairing returns None.
        assert!(hangul_compose(b'a' as u32, 0x0301).is_none());
        // L + L is not a Hangul composition
        assert!(hangul_compose(0x1100, 0x1101).is_none());
    }

    #[test]
    fn is_hangul_syllable_range() {
        assert!(is_hangul_syllable(0xAC00));
        assert!(is_hangul_syllable(0xD7A3)); // last syllable
        assert!(!is_hangul_syllable(0xABFF)); // just before block
        assert!(!is_hangul_syllable(0xD7A4)); // just after block
        assert!(!is_hangul_syllable(0x1100)); // L jamo, not syllable
    }
}
