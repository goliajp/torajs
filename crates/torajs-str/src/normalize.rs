//! Unicode normalization (UAX #15) — P11.6 substrate.
//!
//! Sub-step status:
//! - S1: generator + embed tables in `norm_table.rs` + smoke tests.
//! - S2: `decompose` (recursive canonical / compat + Hangul
//!   algorithmic) + `canonical_order` (CCC stable reorder).
//! - S3: `compose` (canonical primary composite + Hangul L+V /
//!   LV+T) + `normalize` driver that fuses decompose + compose per
//!   UAX #15 §3 D117.
//! - S4 (current): `__torajs_str_normalize` FFI + form parse +
//!   `__torajs_throw_range_error` on invalid form. Driven by SSA
//!   dispatch in `ssa_lower_str::String.prototype.normalize`.
//! - S5: fixture pack + integration ship.

#![allow(dead_code)]

use crate::block::StrBlock;
use crate::norm_table::{lookup_ccc, lookup_composite, lookup_decomp};
use crate::transform::case::{cp_cu_len, decode_cp_at, encode_cp_utf16_le, str_view};
use alloc::vec::Vec;

unsafe extern "C" {
    /// `torajs-throw`'s cross-TU `RangeError` raise (see
    /// `transform::construct` for the same extern). Used by
    /// `__torajs_str_normalize` for `form` values outside the
    /// `{NFC, NFD, NFKC, NFKD}` set per ES §22.1.3.13.
    fn __torajs_throw_range_error(msg: *const u8);
}

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

/// Recursively decompose `cp` and push the result code points into
/// `out`. `compat` selects compatibility decomposition (NFKD/NFKC) vs
/// canonical-only (NFD/NFC). Hangul syllables decompose algorithmically
/// per UAX #15 D118; non-Hangul cps with no decomposition mapping (or
/// with a compat-only entry while `compat = false`) push themselves
/// unchanged.
pub(crate) fn decompose_recurse(cp: u32, out: &mut Vec<u32>, compat: bool) {
    if is_hangul_syllable(cp) {
        hangul_decompose(cp, out);
        return;
    }
    match lookup_decomp(cp) {
        Some((targets, is_compat)) if compat || !is_compat => {
            for &sub in targets {
                decompose_recurse(sub, out, compat);
            }
        }
        _ => out.push(cp),
    }
}

/// Drive `decompose_recurse` over an input cp slice, then apply
/// canonical ordering. Result is in `out`. UAX #15 §3 D68: decompose
/// + canonical order gives NFD (or NFKD if `compat`).
pub(crate) fn decompose(cps: &[u32], compat: bool, out: &mut Vec<u32>) {
    out.clear();
    out.reserve(cps.len());
    for &cp in cps {
        decompose_recurse(cp, out, compat);
    }
    canonical_order(out);
}

/// Stable sort of contiguous CCC>0 runs by ascending CCC. UAX #15 §3
/// D70 (Canonical Ordering Algorithm). Adjacent-swap is O(N) for the
/// short runs found in real text (combining-mark sequences rarely
/// exceed a handful of cps).
pub(crate) fn canonical_order(cps: &mut [u32]) {
    let n = cps.len();
    if n < 2 {
        return;
    }
    let mut i = 0;
    while i < n {
        // Locate a maximal contiguous run of CCC > 0 starting at `i`.
        let start = i;
        while i < n && lookup_ccc(cps[i]) > 0 {
            i += 1;
        }
        let run_len = i - start;
        if run_len > 1 {
            stable_ccc_sort(&mut cps[start..i]);
        }
        // Skip the next starter (CCC == 0) — it terminates the run
        // and cannot be reordered across.
        if i < n {
            i += 1;
        }
    }
}

/// Stable bubble sort by `lookup_ccc`. Stable is required by
/// UAX #15 §3 D70 — equal-CCC marks must keep their relative order.
fn stable_ccc_sort(run: &mut [u32]) {
    let m = run.len();
    for _ in 0..m {
        let mut swapped = false;
        for k in 1..m {
            if lookup_ccc(run[k - 1]) > lookup_ccc(run[k]) {
                run.swap(k - 1, k);
                swapped = true;
            }
        }
        if !swapped {
            break;
        }
    }
}

/// `(a, b) -> composite` primary composite mapping, Hangul-aware.
/// Wraps the table lookup with the algorithmic Hangul L+V / LV+T
/// shortcut. Used by `compose` and exposed for the unit-test surface.
#[inline]
pub(crate) fn primary_composite(a: u32, b: u32) -> Option<u32> {
    if let Some(h) = hangul_compose(a, b) {
        return Some(h);
    }
    lookup_composite(a, b)
}

/// UAX #15 §3 D117 canonical composition. Operates in place on a
/// previously-decomposed + canonical-ordered sequence: pair each
/// combining mark with the last starter when the (starter, mark)
/// primary composite exists AND the mark is not blocked by an
/// intervening same-or-higher-CCC mark.
///
/// `last_starter_out` is the write index of the most recent emitted
/// starter; `max_ccc_since_starter` is the largest CCC observed in
/// the marks between that starter and the read cursor (0 when no
/// marks have been emitted since). A mark `cur` with CCC `c` is
/// blocked iff `c <= max_ccc_since_starter` — in that case it cannot
/// reach the starter, so it must just be re-emitted as-is.
pub(crate) fn compose(cps: &mut Vec<u32>) {
    let n = cps.len();
    if n < 2 {
        return;
    }
    let mut w = 1usize; // write cursor; cps[0] is always kept
    let mut last_starter_w: Option<usize> = if lookup_ccc(cps[0]) == 0 {
        Some(0)
    } else {
        None
    };
    let mut max_ccc_since_starter: u8 = 0;
    for read in 1..n {
        let cur = cps[read];
        let cur_ccc = lookup_ccc(cur);
        let mut combined = false;
        if let Some(starter_w) = last_starter_w {
            let blocked = cur_ccc != 0 && max_ccc_since_starter >= cur_ccc;
            if !blocked {
                if let Some(composite) = primary_composite(cps[starter_w], cur) {
                    cps[starter_w] = composite;
                    combined = true;
                }
            }
        }
        if !combined {
            cps[w] = cur;
            if cur_ccc == 0 {
                last_starter_w = Some(w);
                max_ccc_since_starter = 0;
            } else if cur_ccc > max_ccc_since_starter {
                max_ccc_since_starter = cur_ccc;
            }
            w += 1;
        }
    }
    cps.truncate(w);
}

/// Convenience NFD / NFC / NFKD / NFKC driver: decompose into `out`
/// (canonical or compat per `compat`), then optionally compose
/// (canonical primary composite + Hangul). `compose_phase = false`
/// stops after canonical order = NFD/NFKD; `true` continues to
/// NFC/NFKC.
pub(crate) fn normalize(cps: &[u32], compat: bool, compose_phase: bool, out: &mut Vec<u32>) {
    decompose(cps, compat, out);
    if compose_phase {
        compose(out);
    }
}

/// Form tag for [`__torajs_str_normalize`] / `parse_form`. Order
/// matters only for the (compat, compose) tuple mapping; SSA
/// dispatch does not depend on these values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NormForm {
    Nfc,
    Nfd,
    Nfkc,
    Nfkd,
}

impl NormForm {
    /// `(compat, compose)` per UAX #15 §3 D40-D43:
    /// - NFD  = canonical decompose only
    /// - NFC  = canonical decompose + canonical compose
    /// - NFKD = compat decompose only
    /// - NFKC = compat decompose + canonical compose
    #[inline]
    pub(crate) fn flags(self) -> (bool, bool) {
        match self {
            NormForm::Nfd => (false, false),
            NormForm::Nfc => (false, true),
            NormForm::Nfkd => (true, false),
            NormForm::Nfkc => (true, true),
        }
    }
}

/// Match the bytes of a form Str against the four ASCII literals.
/// `form_payload` is the raw payload of the form Str (Latin-1 or
/// UTF-16 LE per `is_latin1`). Returns `None` for anything else.
pub(crate) fn parse_form(
    form_payload: &[u8],
    form_length: u32,
    is_latin1: bool,
) -> Option<NormForm> {
    // All four valid forms are ASCII-only, so under Latin-1 storage
    // they are a straight byte slice; under UTF-16 LE storage their
    // bytes look like `N\0F\0C\0`. We match both shapes without
    // allocating an intermediate buffer.
    if is_latin1 {
        let bytes = &form_payload[..form_length as usize];
        match bytes {
            b"NFC" => Some(NormForm::Nfc),
            b"NFD" => Some(NormForm::Nfd),
            b"NFKC" => Some(NormForm::Nfkc),
            b"NFKD" => Some(NormForm::Nfkd),
            _ => None,
        }
    } else {
        // UTF-16 LE: each cu is 2 bytes, high byte must be 0 for
        // every ASCII cp; bail otherwise.
        let cu = form_length as usize;
        let bytes = &form_payload[..cu * 2];
        let mut ascii = [0u8; 4];
        if cu > 4 {
            return None;
        }
        for i in 0..cu {
            if bytes[i * 2 + 1] != 0 {
                return None;
            }
            ascii[i] = bytes[i * 2];
        }
        match &ascii[..cu] {
            b"NFC" => Some(NormForm::Nfc),
            b"NFD" => Some(NormForm::Nfd),
            b"NFKC" => Some(NormForm::Nfkc),
            b"NFKD" => Some(NormForm::Nfkd),
            _ => None,
        }
    }
}

/// Encode `out_cps` into a fresh refcount=1 Str block, picking
/// Latin-1 when every cp fits in `u8`, UTF-16 LE otherwise. Used by
/// [`__torajs_str_normalize`]; mirrors `transform::case::build_block`
/// but lives here so `normalize` does not reach into a sibling
/// module's private API.
fn build_normalized_block(out_cps: &[u32]) -> *mut u8 {
    let mut max_cp: u32 = 0;
    let mut out_cu: u32 = 0;
    for &cp in out_cps {
        if cp > max_cp {
            max_cp = cp;
        }
        out_cu += cp_cu_len(cp);
    }
    let out_latin1 = max_cp <= 0xFF;
    let mut block = StrBlock::alloc_with_encoding(out_cu, out_latin1);
    if out_cu == 0 {
        return block.into_raw();
    }
    let byte_cnt = if out_latin1 { out_cu } else { out_cu * 2 };
    let dst = unsafe { block.as_bytes_mut(byte_cnt) };
    if out_latin1 {
        for (i, &cp) in out_cps.iter().enumerate() {
            dst[i] = cp as u8;
        }
    } else {
        let mut buf: Vec<u8> = Vec::with_capacity(byte_cnt as usize);
        for &cp in out_cps {
            encode_cp_utf16_le(cp, &mut buf);
        }
        debug_assert_eq!(buf.len(), byte_cnt as usize);
        dst.copy_from_slice(&buf);
    }
    block.into_raw()
}

/// Decode a Str payload into a `Vec<u32>` of code points. Handles
/// surrogate pair combining for UTF-16 storage so that the cp stream
/// matches the JS `for-of` shape used by the rest of the module.
fn decode_payload_to_cps(payload: &[u8], total_cu: usize, is_latin1: bool) -> Vec<u32> {
    let mut cps: Vec<u32> = Vec::with_capacity(total_cu);
    let mut i = 0;
    while i < total_cu {
        let (cp, adv) = decode_cp_at(payload, i, total_cu, is_latin1);
        cps.push(cp);
        i += adv;
    }
    cps
}

/// `s.normalize(form)` per ES §22.1.3.13. Allocs a fresh
/// refcount=1 Str block holding the normalized form, or — when
/// `form` parses to none of NFC/NFD/NFKC/NFKD — records a pending
/// `RangeError` via `__torajs_throw_range_error` and returns the
/// receiver `s` unchanged (the SSA-emitted `emit_throw_check` at
/// the call site propagates to user `try/catch` before the result
/// is consumed).
///
/// # Safety
///
/// `s` and `form` must each be valid Str heap blocks (or `null` is
/// not accepted — SSA always passes a literal `"NFC"` default when
/// no user arg).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_normalize(s: *const u8, form: *const u8) -> *mut u8 {
    let (form_payload, form_length, form_is_latin1) = unsafe { str_view(form) };
    let parsed = parse_form(form_payload, form_length, form_is_latin1);
    let Some(nf) = parsed else {
        // Invalid form → RangeError. Return a cheap stand-in so
        // the call site (which always uses the result via the
        // throw-check guard) has a non-null pointer. Reusing the
        // receiver matches the spec's "never observe a successful
        // value when throwing" — emit_throw_check kills the path
        // before the value flows downstream.
        unsafe {
            // Message matches bun's so conformance diff-tests can
            // rely on `(e as Error).message`.
            __torajs_throw_range_error(
                b"argument does not match any normalization form\0".as_ptr(),
            );
        }
        return s as *mut u8;
    };
    let (compat, compose_phase) = nf.flags();
    let (payload, length, is_latin1) = unsafe { str_view(s) };
    let cps = decode_payload_to_cps(payload, length as usize, is_latin1);
    let mut out = Vec::new();
    normalize(&cps, compat, compose_phase, &mut out);
    build_normalized_block(&out)
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

    // ---------------- S2: decompose + canonical_order ----------------

    fn dec(cps: &[u32], compat: bool) -> Vec<u32> {
        let mut out = Vec::new();
        decompose(cps, compat, &mut out);
        out
    }

    #[test]
    fn decompose_canonical_e_acute() {
        // é U+00E9 -> e + COMBINING ACUTE (canonical)
        assert_eq!(dec(&[0x00E9], false), vec![0x0065, 0x0301]);
        // compat path produces the same result for canonical-only entries
        assert_eq!(dec(&[0x00E9], true), vec![0x0065, 0x0301]);
    }

    #[test]
    fn decompose_compat_only_gates_on_flag() {
        // ﬁ U+FB01 has only a `<compat>` mapping. Canonical path
        // must leave it alone; compat path must expand to f + i.
        assert_eq!(dec(&[0xFB01], false), vec![0xFB01]);
        assert_eq!(dec(&[0xFB01], true), vec![0x0066, 0x0069]);
    }

    #[test]
    fn decompose_ascii_passthrough() {
        assert_eq!(
            dec(&[b'a' as u32, b'b' as u32, b'c' as u32], false),
            vec![b'a' as u32, b'b' as u32, b'c' as u32]
        );
        assert_eq!(dec(&[], false), Vec::<u32>::new());
    }

    #[test]
    fn decompose_hangul_lv() {
        // 가 U+AC00 -> ᄀ U+1100 + ᅡ U+1161
        assert_eq!(dec(&[0xAC00], false), vec![0x1100, 0x1161]);
    }

    #[test]
    fn decompose_hangul_lvt() {
        // 각 U+AC01 -> ᄀ + ᅡ + ᆨ (T = U+11A8)
        assert_eq!(dec(&[0xAC01], false), vec![0x1100, 0x1161, 0x11A8]);
    }

    #[test]
    fn decompose_recursive_chain() {
        // Å U+00C5 (LATIN CAPITAL LETTER A WITH RING ABOVE) ->
        // A (U+0041) + COMBINING RING ABOVE (U+030A). One level.
        assert_eq!(dec(&[0x00C5], false), vec![0x0041, 0x030A]);
        // Ǻ U+01FA (... A WITH RING ABOVE AND ACUTE) -> Å + acute
        // -> A + ring + acute (recursive). Canonical ordering then
        // sorts ring(230) before acute(230) — same CCC so stable
        // preserves source order (ring then acute).
        assert_eq!(dec(&[0x01FA], false), vec![0x0041, 0x030A, 0x0301]);
    }

    #[test]
    fn canonical_order_swaps_combining_marks() {
        // U+0327 COMBINING CEDILLA = CCC 202 (Below)
        // U+0301 COMBINING ACUTE   = CCC 230 (Above)
        // Source order: a, acute, cedilla -> ordered: a, cedilla, acute
        // (cedilla 202 < acute 230 so cedilla comes first).
        let mut cps = vec![b'a' as u32, 0x0301, 0x0327];
        canonical_order(&mut cps);
        assert_eq!(cps, vec![b'a' as u32, 0x0327, 0x0301]);
    }

    #[test]
    fn canonical_order_stable_equal_ccc() {
        // Two CCC=230 marks (acute U+0301 and grave U+0300): stable
        // sort must preserve source order.
        let mut cps = vec![b'a' as u32, 0x0301, 0x0300];
        canonical_order(&mut cps);
        assert_eq!(cps, vec![b'a' as u32, 0x0301, 0x0300]);
        let mut cps = vec![b'a' as u32, 0x0300, 0x0301];
        canonical_order(&mut cps);
        assert_eq!(cps, vec![b'a' as u32, 0x0300, 0x0301]);
    }

    #[test]
    fn canonical_order_starter_blocks_reorder() {
        // Two combining runs separated by a starter must not be
        // reordered across. a + acute(230) + b + cedilla(202):
        // acute is its own 1-mark run; cedilla is its own 1-mark
        // run; neither needs swapping.
        let mut cps = vec![b'a' as u32, 0x0301, b'b' as u32, 0x0327];
        canonical_order(&mut cps);
        assert_eq!(cps, vec![b'a' as u32, 0x0301, b'b' as u32, 0x0327]);
    }

    #[test]
    fn decompose_combines_recursion_and_reorder() {
        // Source: Å (U+00C5) + COMBINING CEDILLA (U+0327, CCC 202)
        // Recurse Å -> A + ring (CCC 230). Then run = [ring(230),
        // cedilla(202)] which sorts to [cedilla, ring].
        assert_eq!(dec(&[0x00C5, 0x0327], false), vec![0x0041, 0x0327, 0x030A]);
    }

    #[test]
    fn decompose_empty_and_starter_only() {
        assert_eq!(dec(&[], false), Vec::<u32>::new());
        // All starters, no marks — nothing to reorder.
        let src = vec![b'h' as u32, b'i' as u32, 0x4E2D, 0x6587];
        assert_eq!(dec(&src, false), src);
    }

    // ---------------- S3: compose + normalize driver ----------------

    fn comp(cps: &[u32]) -> Vec<u32> {
        let mut v = cps.to_vec();
        compose(&mut v);
        v
    }

    fn nfc(cps: &[u32]) -> Vec<u32> {
        let mut out = Vec::new();
        normalize(cps, false, true, &mut out);
        out
    }

    fn nfkc(cps: &[u32]) -> Vec<u32> {
        let mut out = Vec::new();
        normalize(cps, true, true, &mut out);
        out
    }

    #[test]
    fn primary_composite_canonical_pair() {
        assert_eq!(primary_composite(0x0065, 0x0301), Some(0x00E9));
        assert_eq!(primary_composite(0x0041, 0x0301), Some(0x00C1));
    }

    #[test]
    fn primary_composite_hangul_l_v() {
        // ᄀ + ᅡ -> 가
        assert_eq!(primary_composite(0x1100, 0x1161), Some(0xAC00));
    }

    #[test]
    fn primary_composite_hangul_lv_t() {
        // 가 + ᆨ -> 각
        assert_eq!(primary_composite(0xAC00, 0x11A8), Some(0xAC01));
    }

    #[test]
    fn compose_canonical_e_acute_in_place() {
        assert_eq!(comp(&[0x0065, 0x0301]), vec![0x00E9]);
    }

    #[test]
    fn compose_no_starter_passthrough() {
        // Starts with a combining mark — no preceding starter to bind
        // to; mark must pass through unchanged.
        assert_eq!(comp(&[0x0301]), vec![0x0301]);
        // Two combining marks alone.
        assert_eq!(comp(&[0x0301, 0x0327]), vec![0x0301, 0x0327]);
    }

    #[test]
    fn compose_blocked_by_higher_ccc() {
        // e + ring(230) + acute(230): ring is not blocked, but (e,
        // ring) has no composite in the table (this pair does not
        // form a precomposed cp). Acute is blocked: max_ccc since
        // starter = 230 (ring) and acute's CCC is 230 (not strictly
        // greater), so it cannot reach e.
        assert_eq!(
            comp(&[0x0065, 0x030A, 0x0301]),
            vec![0x0065, 0x030A, 0x0301]
        );
    }

    #[test]
    fn compose_hangul_round_trips() {
        // L + V -> LV
        assert_eq!(comp(&[0x1100, 0x1161]), vec![0xAC00]);
        // L + V + T -> LVT (first L+V composes to LV, then LV+T)
        assert_eq!(comp(&[0x1100, 0x1161, 0x11A8]), vec![0xAC01]);
    }

    #[test]
    fn compose_starter_resets_run() {
        // a + acute(230) (composes to á) + b + acute(230) (composes
        // to b + acute = b́; b + acute has no composite, so it stays).
        // Verifies a starter resets max_ccc tracking.
        let out = comp(&[b'a' as u32, 0x0301, b'b' as u32, 0x0301]);
        assert_eq!(out, vec![0x00E1, b'b' as u32, 0x0301]);
    }

    #[test]
    fn nfc_idempotent_on_precomposed() {
        // NFC of already-NFC input should round-trip.
        assert_eq!(nfc(&[0x00E9]), vec![0x00E9]); // é
        assert_eq!(
            nfc(&[b'h' as u32, b'i' as u32]),
            vec![b'h' as u32, b'i' as u32]
        );
    }

    #[test]
    fn nfc_recomposes_decomposed_input() {
        // NFC of decomposed input -> composed.
        assert_eq!(nfc(&[0x0065, 0x0301]), vec![0x00E9]);
        // Hangul jamo -> syllable.
        assert_eq!(nfc(&[0x1100, 0x1161]), vec![0xAC00]);
        assert_eq!(nfc(&[0x1100, 0x1161, 0x11A8]), vec![0xAC01]);
    }

    #[test]
    fn nfc_ignores_compat_mapping() {
        // ﬁ (U+FB01) only has a `<compat>` decomposition. NFC must
        // not expand it; result stays as ﬁ.
        assert_eq!(nfc(&[0xFB01]), vec![0xFB01]);
    }

    #[test]
    fn nfkc_expands_and_composes() {
        // ﬁ -> NFKD -> f + i. There is no f+i composite, so NFKC
        // result is f + i.
        assert_eq!(nfkc(&[0xFB01]), vec![0x0066, 0x0069]);
        // µ (U+00B5 MICRO SIGN) -> NFKD -> μ (U+03BC GREEK SMALL
        // LETTER MU). No further composition.
        assert_eq!(nfkc(&[0x00B5]), vec![0x03BC]);
    }

    #[test]
    fn compose_canonical_reorder_then_compose() {
        // Source (decomposed-with-out-of-order marks):
        //   a + acute(230) + cedilla(202)
        // Canonical order swaps to: a + cedilla(202) + acute(230)
        // Compose: a + cedilla -> no composite (a-cedilla is not a
        // precomposed cp in Latin-1), so cedilla stays. Then acute
        // is blocked by cedilla? No — cedilla CCC=202 < acute CCC=230,
        // so acute is NOT blocked. But (a, acute) has composite á...
        // Wait: cedilla is BETWEEN a and acute in the post-reorder
        // string, so max_ccc_since_starter = 202 when we get to
        // acute. acute_ccc(230) > 202 => not blocked. So acute can
        // still compose with a -> á + cedilla.
        assert_eq!(nfc(&[b'a' as u32, 0x0301, 0x0327]), vec![0x00E1, 0x0327]);
    }
}
