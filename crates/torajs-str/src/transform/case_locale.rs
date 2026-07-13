//! Locale-tailored case folding — `s.toLocaleUpperCase(locale)` /
//! `s.toLocaleLowerCase(locale)` per ES402 sup-string.prototype.
//! tolocale{upper,lower}case.
//!
//! Implements the SpecialCasing.txt conditional, language-sensitive
//! mappings for the three tailored locales the UCD defines:
//!
//! - **tr / az (Turkic)** — lower: `İ (U+0130) -> i`, `U+0307`
//!   deleted After_I, `I -> ı (U+0131)` when Not_Before_Dot;
//!   upper: `i -> İ`.
//! - **lt (Lithuanian)** — lower: `I/J/Į` gain a combining dot
//!   above (U+0307) when More_Above; `Ì/Í/Ĩ` expand to
//!   `i + U+0307 + accent`; upper: `U+0307` deleted
//!   After_Soft_Dotted.
//!
//! Context rules per Unicode Table 3-17 (UAX #21): all four
//! conditions scan over code points whose combining class is
//! neither 0 nor 230 and stop at the first cp whose ccc is 0 or
//! 230 ([`is_ccc_zero`] / [`is_ccc_above`] from the generated
//! tables; Soft_Dotted from PropList.txt).
//!
//! Any other locale (including `und` / empty = host default) takes
//! the Default Case Conversion path in [`super::case`]. Locale
//! identifier *validation* (BCP47 / CanonicalizeLocaleList
//! RangeError) is the follow-up cut — this module only selects the
//! tailoring.
//!
//! IR-side surface: `__torajs_str_to_locale_upper(s, locale)` /
//! `__torajs_str_to_locale_lower(s, locale)` (typed tier, both
//! `(Str, Str) -> Str`) and `__torajs_str_any_locale_case(s,
//! locale, upper)` (any tier, nullable locale).

use crate::case_table::{is_ccc_above, is_ccc_zero, is_soft_dotted};

use super::case::{collect_mapped, str_view};

/// Which tailored-casing rule set applies. Parsed from the locale
/// argument's primary language subtag.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaseLocale {
    /// Default Case Conversion (en-US / und / everything else).
    Default,
    /// Turkish + Azeri dotted/dotless-I tailoring.
    Turkic,
    /// Lithuanian dot-above tailoring.
    Lithuanian,
}

/// Walk backward from `idx`, skipping cps whose ccc is neither 0
/// nor 230. Returns the first cp whose ccc is 0 or 230, if any.
#[inline]
fn scan_back(src: &[u32], idx: usize) -> Option<u32> {
    let mut i = idx;
    while i > 0 {
        i -= 1;
        let cp = src[i];
        if is_ccc_zero(cp) || is_ccc_above(cp) {
            return Some(cp);
        }
    }
    None
}

/// Walk forward from `idx`, skipping cps whose ccc is neither 0
/// nor 230. Returns the first cp whose ccc is 0 or 230, if any.
#[inline]
fn scan_fwd(src: &[u32], idx: usize) -> Option<u32> {
    for &cp in &src[idx + 1..] {
        if is_ccc_zero(cp) || is_ccc_above(cp) {
            return Some(cp);
        }
    }
    None
}

/// COMBINING DOT ABOVE.
const DOT_ABOVE: u32 = 0x0307;

/// After_I: the last preceding base-or-above cp is an uppercase I.
#[inline]
fn after_i(src: &[u32], idx: usize) -> bool {
    scan_back(src, idx) == Some(0x0049)
}

/// Not_Before_Dot: the next base-or-above cp is NOT U+0307.
#[inline]
fn not_before_dot(src: &[u32], idx: usize) -> bool {
    scan_fwd(src, idx) != Some(DOT_ABOVE)
}

/// More_Above: the next base-or-above cp exists and has ccc 230.
#[inline]
fn more_above(src: &[u32], idx: usize) -> bool {
    matches!(scan_fwd(src, idx), Some(cp) if is_ccc_above(cp))
}

/// After_Soft_Dotted: the last preceding base-or-above cp carries
/// the Soft_Dotted property.
#[inline]
fn after_soft_dotted(src: &[u32], idx: usize) -> bool {
    matches!(scan_back(src, idx), Some(cp) if is_soft_dotted(cp))
}

/// SpecialCasing.txt conditional, language-sensitive entries.
/// Returns `Some(mapped)` when a tailored rule fires for
/// `src[idx]` (an empty slice = the cp is deleted); `None` falls
/// through to Default Case Conversion.
pub(crate) fn map_cp_tailored(
    src: &[u32],
    idx: usize,
    upper: bool,
    loc: CaseLocale,
) -> Option<&'static [u32]> {
    let cp = src[idx];
    match (loc, upper, cp) {
        // 0130; 0069; ...; tr/az — İ lowers to plain i (the
        // unconditional default expands to i + U+0307).
        (CaseLocale::Turkic, false, 0x0130) => Some(&[0x0069]),
        // 0307; ; ...; tr/az After_I — the dot above is consumed
        // by the I -> i mapping.
        (CaseLocale::Turkic, false, DOT_ABOVE) if after_i(src, idx) => Some(&[]),
        // 0049; 0131; ...; tr/az Not_Before_Dot — I lowers to
        // dotless ı unless a dot above follows (then the default
        // I -> i applies and After_I eats the dot).
        (CaseLocale::Turkic, false, 0x0049) if not_before_dot(src, idx) => Some(&[0x0131]),
        // 0069; ...; 0130; tr/az — i uppercases to İ.
        (CaseLocale::Turkic, true, 0x0069) => Some(&[0x0130]),
        // 0049/004A/012E; +0307; lt More_Above — retain the dot
        // when lowercasing i/j/į under a following above-accent.
        (CaseLocale::Lithuanian, false, 0x0049) if more_above(src, idx) => {
            Some(&[0x0069, DOT_ABOVE])
        }
        (CaseLocale::Lithuanian, false, 0x004A) if more_above(src, idx) => {
            Some(&[0x006A, DOT_ABOVE])
        }
        (CaseLocale::Lithuanian, false, 0x012E) if more_above(src, idx) => {
            Some(&[0x012F, DOT_ABOVE])
        }
        // 00CC/00CD/0128; lt — accented capital I lowers to
        // i + dot above + accent (unconditional in lt).
        (CaseLocale::Lithuanian, false, 0x00CC) => Some(&[0x0069, DOT_ABOVE, 0x0300]),
        (CaseLocale::Lithuanian, false, 0x00CD) => Some(&[0x0069, DOT_ABOVE, 0x0301]),
        (CaseLocale::Lithuanian, false, 0x0128) => Some(&[0x0069, DOT_ABOVE, 0x0303]),
        // 0307; 0307; ; ; lt After_Soft_Dotted — upper/title
        // delete the dot above following a soft-dotted base.
        (CaseLocale::Lithuanian, true, DOT_ABOVE) if after_soft_dotted(src, idx) => Some(&[]),
        _ => None,
    }
}

/// Parse a locale identifier Str block into the tailoring bucket:
/// the primary language subtag (up to the first `-`), compared
/// ASCII-case-insensitively against `tr` / `az` / `lt`. Everything
/// else — including the empty string (host default) and `und` —
/// answers `Default`.
///
/// # Safety
///
/// `locale` must be a valid Str heap block.
unsafe fn parse_case_locale(locale: *const u8) -> CaseLocale {
    let (payload, length, is_latin1) = unsafe { str_view(locale) };
    // The primary subtag of a tailored locale is exactly 2 ASCII
    // letters; read code units encoding-agnostically.
    let unit_at = |i: usize| -> u32 {
        if is_latin1 {
            payload[i] as u32
        } else {
            (payload[i * 2] as u32) | ((payload[i * 2 + 1] as u32) << 8)
        }
    };
    let len = length as usize;
    if len != 2 && !(len > 2 && unit_at(2) == b'-' as u32) {
        return CaseLocale::Default;
    }
    let (a, b) = (unit_at(0) | 0x20, unit_at(1) | 0x20);
    match (a, b) {
        (0x74, 0x72) | (0x61, 0x7A) => CaseLocale::Turkic, // tr | az
        (0x6C, 0x74) => CaseLocale::Lithuanian,            // lt
        _ => CaseLocale::Default,
    }
}

/// Shared core for the two typed-tier FFI entry points.
unsafe fn to_locale_case(s: *const u8, locale: *const u8, upper: bool) -> *mut u8 {
    let loc = if locale.is_null() {
        CaseLocale::Default
    } else {
        unsafe { parse_case_locale(locale) }
    };
    let (payload, length, is_latin1) = unsafe { str_view(s) };
    let (out_cps, max_cp, out_cu) = collect_mapped(payload, length as usize, is_latin1, upper, loc);
    super::case::build_block(&out_cps, max_cp, out_cu)
}

/// `s.toLocaleUpperCase(locale)` — locale-tailored uppercase per
/// ES402. Allocs a fresh refcount=1 Str block.
///
/// # Safety
///
/// `s` and `locale` are valid Str heap blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_to_locale_upper(s: *const u8, locale: *const u8) -> *mut u8 {
    unsafe { to_locale_case(s, locale, true) }
}

/// `s.toLocaleLowerCase(locale)` — locale-tailored lowercase per
/// ES402. Allocs a fresh refcount=1 Str block.
///
/// # Safety
///
/// See [`__torajs_str_to_locale_upper`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_to_locale_lower(s: *const u8, locale: *const u8) -> *mut u8 {
    unsafe { to_locale_case(s, locale, false) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn low(src: &[u32], loc: CaseLocale) -> alloc::vec::Vec<u32> {
        fold(src, false, loc)
    }

    fn up(src: &[u32], loc: CaseLocale) -> alloc::vec::Vec<u32> {
        fold(src, true, loc)
    }

    /// Drive map_cp_tailored over a cp sequence the way
    /// collect_mapped does, falling back to identity for cps the
    /// tailored table skips (enough for these context tests — no
    /// default-table dependence).
    fn fold(src: &[u32], upper: bool, loc: CaseLocale) -> alloc::vec::Vec<u32> {
        let mut out = alloc::vec::Vec::new();
        for idx in 0..src.len() {
            match map_cp_tailored(src, idx, upper, loc) {
                Some(mapped) => out.extend_from_slice(mapped),
                None => out.push(src[idx]),
            }
        }
        out
    }

    #[test]
    fn turkic_dotted_capital_i_lowers_to_plain_i() {
        assert_eq!(low(&[0x0130], CaseLocale::Turkic), [0x0069]);
    }

    #[test]
    fn turkic_after_i_deletes_dot_above() {
        // "İ" -> "i" (I lowers via default i since a dot
        // follows; the dot itself is deleted).
        let src = [0x0049, 0x0307];
        assert!(map_cp_tailored(&src, 0, false, CaseLocale::Turkic).is_none());
        assert_eq!(
            map_cp_tailored(&src, 1, false, CaseLocale::Turkic),
            Some(&[][..])
        );
    }

    #[test]
    fn turkic_after_i_skips_ccc_220() {
        // I, dot-below (ccc 220), dot-above: the dot-above is still
        // After_I (220 is skipped), I itself is Before_Dot.
        let src = [0x0049, 0x0323, 0x0307];
        assert!(map_cp_tailored(&src, 0, false, CaseLocale::Turkic).is_none());
        assert_eq!(
            map_cp_tailored(&src, 2, false, CaseLocale::Turkic),
            Some(&[][..])
        );
    }

    #[test]
    fn turkic_ccc_230_blocks_after_i() {
        // I, grave (ccc 230), dot-above: the grave interrupts —
        // I is Not_Before_Dot -> dotless; the dot survives.
        let src = [0x0049, 0x0300, 0x0307];
        assert_eq!(
            map_cp_tailored(&src, 0, false, CaseLocale::Turkic),
            Some(&[0x0131][..])
        );
        assert!(map_cp_tailored(&src, 2, false, CaseLocale::Turkic).is_none());
    }

    #[test]
    fn turkic_bare_i_round_trip() {
        assert_eq!(low(&[0x0049], CaseLocale::Turkic), [0x0131]);
        assert_eq!(up(&[0x0069], CaseLocale::Turkic), [0x0130]);
    }

    #[test]
    fn lithuanian_more_above_adds_dot() {
        assert_eq!(
            low(&[0x0049, 0x0300], CaseLocale::Lithuanian),
            [0x0069, 0x0307, 0x0300]
        );
        // Intervening ccc-220 mark does not block More_Above.
        assert_eq!(
            low(&[0x004A, 0x0325, 0x0300], CaseLocale::Lithuanian),
            [0x006A, 0x0307, 0x0325, 0x0300]
        );
        // No above-accent following: no dot inserted.
        assert!(map_cp_tailored(&[0x0049, 0x0077], 0, false, CaseLocale::Lithuanian).is_none());
    }

    #[test]
    fn lithuanian_after_soft_dotted_deletes_dot_on_upper() {
        // i + dot-above uppercases to bare I.
        let src = [0x0069, 0x0307];
        assert_eq!(
            map_cp_tailored(&src, 1, true, CaseLocale::Lithuanian),
            Some(&[][..])
        );
        // Capital I is not Soft_Dotted: dot survives.
        assert!(map_cp_tailored(&[0x0049, 0x0307], 1, true, CaseLocale::Lithuanian).is_none());
        // Supplementary-plane soft-dotted base (math italic small i).
        assert_eq!(
            map_cp_tailored(&[0x1D456, 0x0307], 1, true, CaseLocale::Lithuanian),
            Some(&[][..])
        );
    }
}
