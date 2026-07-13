//! Str case-folding — `s.toUpperCase()` / `s.toLowerCase()` per
//! ES §22.1.3.29 / §22.1.3.30 (Default Case Conversion).
//!
//! P11.5-A1 — full Unicode default case fold using UCD 16.0
//! UnicodeData.txt (Simple mapping) + SpecialCasing.txt
//! Unconditional Full mapping. Tables in [`crate::case_table`],
//! regenerated via `scripts/ucd/gen_case_tables.py`.
//!
//! Spec coverage:
//! - Simple 1-cp -> 1-cp (Greek, Cyrillic, Latin Extended, Armenian, ...)
//! - Full 1-cp -> N-cp expansion (`ß -> SS`, `ﬃ -> FFI`, `İ -> i̇`)
//! - Encoding-aware: output picks canonical Latin-1 vs UTF-16 by
//!   scanning all mapped code points for max value
//! - Surrogate-pair-aware: supplementary plane cps read via
//!   `__torajs_str_code_point_at` semantics
//!
//! P11.5-A3+A4 — Final_Sigma context-dependent mapping per UAX #21:
//! when `Σ (U+03A3)` appears in a lowercase fold with a preceding
//! Cased letter and no following Cased letter (Case_Ignorable code
//! points skipped on both sides per UAX #21), it maps to `ς
//! (U+03C2)` instead of the default `σ (U+03C3)`. Skip rule covers
//! `"A.Σ"` -> `"a.ς"` (period is Case_Ignorable), `"ÁΣ"` -> `"áς"`
//! (combining acute is Case_Ignorable), etc.
//!
//! NOT covered (Locale follow-up):
//! - Locale-tailored mappings (Turkish dotless ı, Lithuanian, etc.)
//!
//! IR-side surface (declared in `ssa_lower::lower` and consumed by
//! the `toUpperCase` / `toLowerCase` method dispatch in
//! `lower_expr`; alloc-intrinsic noalias-whitelisted on the
//! LLVM-era backend): `__torajs_str_to_upper(s)`
//! and `__torajs_str_to_lower(s)`, both `Str -> Str`.

use alloc::vec::Vec;

use crate::block::StrBlock;
use crate::case_table::{
    FULL_LOWER, FULL_UPPER, SIMPLE_LOWER, SIMPLE_UPPER, full_lookup, is_case_ignorable, is_cased,
    simple_lookup,
};
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use torajs_rc::HeapHeader;

// ============================================================
// Layout-aware FFI helpers (sub-module-local; see mod.rs for why)
// ============================================================

#[inline]
pub(crate) unsafe fn str_view<'a>(p: *const u8) -> (&'a [u8], u32, bool) {
    let length = unsafe { (p.add(STR_LEN_OFF) as *const u32).read() };
    let header = unsafe { &*(p as *const HeapHeader) };
    let is_latin1 = (header.flags & STR_FLAG_IS_LATIN1) != 0;
    let byte_cnt = if is_latin1 {
        length as usize
    } else {
        (length as usize) * 2
    };
    let payload = unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), byte_cnt) };
    (payload, length, is_latin1)
}

// ============================================================
// Pure-Rust cores
// ============================================================

/// ASCII upper-fold from `src` into `dst`. Both slices must be the
/// same length; this is a single linear pass with no branches on
/// the dominant ASCII-uppercase / ASCII-non-letter input.
#[inline]
pub fn to_upper_into(src: &[u8], dst: &mut [u8]) {
    debug_assert_eq!(src.len(), dst.len());
    for (i, &c) in src.iter().enumerate() {
        // Branchless idiom: `c.is_ascii_lowercase()` is the range
        // check `'a'..='z'`. Subtracting 32 maps to upper; the cmp
        // result is folded into a conditional move by LLVM at -O3.
        dst[i] = if c.is_ascii_lowercase() { c - 32 } else { c };
    }
}

/// ASCII lower-fold from `src` into `dst`. Mirror of
/// [`to_upper_into`].
#[inline]
pub fn to_lower_into(src: &[u8], dst: &mut [u8]) {
    debug_assert_eq!(src.len(), dst.len());
    for (i, &c) in src.iter().enumerate() {
        dst[i] = if c.is_ascii_uppercase() { c + 32 } else { c };
    }
}

// ============================================================
// extern "C" wrappers — preserve pre-rewrite ABI bit-for-bit
// ============================================================

/// `s.toUpperCase()` — ASCII fold, single pool-aware alloc.
///
/// # Safety
///
/// `s` must be a valid Str heap block (header + len at offsets
/// `0..STR_DATA_OFF`, then `len` payload bytes). The returned
/// pointer is a fresh refcount=1 Str block; ownership transfers
/// to the caller.
/// Decode one code point from `payload` starting at code-unit index
/// `cu_idx`. Returns `(cp, cu_advance)` where `cu_advance` is 1 for
/// BMP and 2 for a high+low surrogate pair. Mirrors the
/// `combine_or_lone` shape in `crate::code_point` but operates on
/// raw payload bytes so we don't depend on Str-block layout here.
#[inline]
pub(crate) fn decode_cp_at(
    payload: &[u8],
    cu_idx: usize,
    total_cu: usize,
    is_latin1: bool,
) -> (u32, usize) {
    if is_latin1 {
        (payload[cu_idx] as u32, 1)
    } else {
        let off = cu_idx * 2;
        let lo = payload[off] as u16;
        let hi = payload[off + 1] as u16;
        let unit = (((hi << 8) | lo) as u32) & 0xFFFF;
        if (0xD800..=0xDBFF).contains(&unit) && cu_idx + 1 < total_cu {
            let off2 = (cu_idx + 1) * 2;
            let lo2 = payload[off2] as u16;
            let hi2 = payload[off2 + 1] as u16;
            let unit2 = (((hi2 << 8) | lo2) as u32) & 0xFFFF;
            if (0xDC00..=0xDFFF).contains(&unit2) {
                let cp = 0x10000 + ((unit - 0xD800) << 10) + (unit2 - 0xDC00);
                return (cp, 2);
            }
        }
        (unit, 1)
    }
}

/// How many UTF-16 code units does `cp` occupy? 2 for supplementary
/// plane, 1 otherwise.
#[inline]
pub(crate) fn cp_cu_len(cp: u32) -> u32 {
    if cp > 0xFFFF { 2 } else { 1 }
}

/// Apply the case fold of `cp` per the requested direction. Returns
/// a slice of mapped code points. Order:
///   1. Full mapping (Unconditional SpecialCasing) — may expand 1 -> N
///   2. Simple mapping (UnicodeData) — always 1 -> 1
///   3. Identity — cp unchanged
///
/// Returned slice is borrowed from either the static table (Full /
/// Simple cases) or the one-element holder slice. Caller stores the
/// values out before reusing the holder.
#[inline]
fn map_cp<'a>(cp: u32, upper: bool, holder: &'a mut [u32; 1]) -> &'a [u32] {
    let full_table = if upper { FULL_UPPER } else { FULL_LOWER };
    if let Some(seq) = full_lookup(full_table, cp) {
        return seq;
    }
    let simple_table = if upper { SIMPLE_UPPER } else { SIMPLE_LOWER };
    if let Some(s) = simple_lookup(simple_table, cp) {
        holder[0] = s;
        return &holder[..];
    }
    holder[0] = cp;
    &holder[..]
}

/// Encode `cp` as UTF-16 LE bytes onto `out` (2 or 4 bytes
/// depending on plane).
#[inline]
pub(crate) fn encode_cp_utf16_le(cp: u32, out: &mut Vec<u8>) {
    if cp <= 0xFFFF {
        let unit = cp as u16;
        out.push((unit & 0xFF) as u8);
        out.push((unit >> 8) as u8);
    } else {
        let offset = cp - 0x10000;
        let high = (0xD800 + (offset >> 10)) as u16;
        let low = (0xDC00 + (offset & 0x3FF)) as u16;
        out.push((high & 0xFF) as u8);
        out.push((high >> 8) as u8);
        out.push((low & 0xFF) as u8);
        out.push((low >> 8) as u8);
    }
}

/// Code point for GREEK CAPITAL LETTER SIGMA.
const SIGMA_UPPER: u32 = 0x03A3;
/// Code point for GREEK SMALL LETTER FINAL SIGMA (word-final form).
const SIGMA_FINAL: u32 = 0x03C2;

/// True iff `cps[idx]` is a Σ that satisfies the Final_Sigma context
/// rule per UAX #21: preceded by a Cased letter (skipping
/// Case_Ignorable) and NOT followed by a Cased letter (skipping
/// Case_Ignorable). Examples:
///   - `"A.Σ"` (Σ at idx 2): prev=`.` (CI), skip back -> `A` (Cased) -> preceded ✓;
///     no chars after -> not followed -> Final ✓
///   - `"A Σ"` (Σ at idx 2): prev=` ` (Zs, NOT CI, NOT Cased) -> not preceded ✗
///   - `"A.Σ.A"`: preceded via `.`->A; followed via `.`->A; both -> NOT Final
#[inline]
fn is_final_sigma(cps: &[u32], idx: usize) -> bool {
    if cps[idx] != SIGMA_UPPER {
        return false;
    }
    if !preceded_by_cased(cps, idx) {
        return false;
    }
    !followed_by_cased(cps, idx)
}

/// Walk `cps[..idx]` from the position just before `idx` backward,
/// skipping Case_Ignorable code points. Returns true if the first
/// non-Case_Ignorable code point encountered is Cased.
#[inline]
fn preceded_by_cased(cps: &[u32], idx: usize) -> bool {
    let mut i = idx;
    while i > 0 {
        i -= 1;
        let cp = cps[i];
        if is_case_ignorable(cp) {
            continue;
        }
        return is_cased(cp);
    }
    false
}

/// Walk `cps[idx + 1..]` forward, skipping Case_Ignorable code
/// points. Returns true if the first non-Case_Ignorable code point
/// encountered is Cased.
#[inline]
fn followed_by_cased(cps: &[u32], idx: usize) -> bool {
    for &cp in &cps[idx + 1..] {
        if is_case_ignorable(cp) {
            continue;
        }
        return is_cased(cp);
    }
    false
}

/// Walk `(payload, total_cu, is_latin1)` decoding each code point,
/// mapping it via [`map_cp`] (or the Final_Sigma override on Σ in
/// lowercase mode, or the locale-tailored SpecialCasing rules when
/// `loc != Default` — see [`super::case_locale`]), and collecting
/// the mapped cps + their max value (for output-encoding decision)
/// + their total cu count.
pub(crate) fn collect_mapped(
    payload: &[u8],
    total_cu: usize,
    is_latin1: bool,
    upper: bool,
    loc: super::case_locale::CaseLocale,
) -> (Vec<u32>, u32, u32) {
    // Pass 1 — decode source code points into a contiguous Vec so
    // the Final_Sigma check can look at adjacent source positions
    // (NOT post-mapping positions: the spec rule is over input).
    let mut src_cps: Vec<u32> = Vec::with_capacity(total_cu);
    let mut i = 0;
    while i < total_cu {
        let (cp, adv) = decode_cp_at(payload, i, total_cu, is_latin1);
        src_cps.push(cp);
        i += adv;
    }

    let mut out_cps: Vec<u32> = Vec::with_capacity(src_cps.len());
    let mut max_cp: u32 = 0;
    let mut out_cu: u32 = 0;
    for (idx, &cp) in src_cps.iter().enumerate() {
        let mut holder: [u32; 1] = [0];
        let tailored = if loc == super::case_locale::CaseLocale::Default {
            None
        } else {
            super::case_locale::map_cp_tailored(&src_cps, idx, upper, loc)
        };
        let mapped: &[u32] = if let Some(t) = tailored {
            t
        } else if !upper && is_final_sigma(&src_cps, idx) {
            holder[0] = SIGMA_FINAL;
            &holder[..]
        } else {
            map_cp(cp, upper, &mut holder)
        };
        for &mc in mapped {
            out_cps.push(mc);
            if mc > max_cp {
                max_cp = mc;
            }
            out_cu += cp_cu_len(mc);
        }
    }
    (out_cps, max_cp, out_cu)
}

/// Encode `out_cps` into a fresh StrBlock. Picks Latin-1 if every
/// cp ≤ 0xFF (canonical encoding invariant: payload bytes match
/// the Latin-1 code-unit values 1:1); otherwise UTF-16 LE.
pub(crate) fn build_block(out_cps: &[u32], max_cp: u32, out_cu: u32) -> *mut u8 {
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

/// `s.toUpperCase()` per ES §22.1.3.29 — UCD Default Case Conversion
/// (Full + Simple Unconditional). Encoding-aware output canonical
/// alloc. Allocs a fresh refcount=1 Str block.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_to_upper(s: *const u8) -> *mut u8 {
    let (payload, length, is_latin1) = unsafe { str_view(s) };
    let (out_cps, max_cp, out_cu) = collect_mapped(
        payload,
        length as usize,
        is_latin1,
        true,
        super::case_locale::CaseLocale::Default,
    );
    build_block(&out_cps, max_cp, out_cu)
}

/// `s.toLowerCase()` per ES §22.1.3.30 — same shape as
/// [`__torajs_str_to_upper`] but reads the lowercase tables.
///
/// # Safety
///
/// See [`__torajs_str_to_upper`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_to_lower(s: *const u8) -> *mut u8 {
    let (payload, length, is_latin1) = unsafe { str_view(s) };
    let (out_cps, max_cp, out_cu) = collect_mapped(
        payload,
        length as usize,
        is_latin1,
        false,
        super::case_locale::CaseLocale::Default,
    );
    build_block(&out_cps, max_cp, out_cu)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn upper_into_basic_ascii() {
        let mut dst = [0u8; 5];
        to_upper_into(b"hello", &mut dst);
        assert_eq!(&dst, b"HELLO");
    }

    #[test]
    fn upper_into_preserves_already_upper_and_non_letter() {
        let mut dst = [0u8; 11];
        to_upper_into(b"Hi! 123 \xFFxy", &mut dst);
        assert_eq!(&dst, b"HI! 123 \xFFXY");
    }

    #[test]
    fn upper_into_passes_through_non_ascii_bytes() {
        // 'é' (UTF-8: C3 A9) must NOT case-fold; both bytes are >= 0x80.
        let mut dst = [0u8; 5];
        to_upper_into(b"\xC3\xA9foo", &mut dst);
        assert_eq!(&dst, b"\xC3\xA9FOO");
    }

    #[test]
    fn lower_into_basic_ascii() {
        let mut dst = [0u8; 5];
        to_lower_into(b"HELLO", &mut dst);
        assert_eq!(&dst, b"hello");
    }

    #[test]
    fn lower_into_preserves_already_lower_and_non_letter() {
        let mut dst = [0u8; 11];
        to_lower_into(b"Hi! 123 \xFFxy", &mut dst);
        assert_eq!(&dst, b"hi! 123 \xFFxy");
    }

    #[test]
    fn lower_into_passes_through_non_ascii_bytes() {
        let mut dst = [0u8; 5];
        to_lower_into(b"\xC3\x89FOO", &mut dst);
        assert_eq!(&dst, b"\xC3\x89foo");
    }

    #[test]
    fn empty_input_yields_empty_output() {
        let mut dst = [0u8; 0];
        to_upper_into(b"", &mut dst);
        to_lower_into(b"", &mut dst);
        assert!(dst.is_empty());
    }

    #[test]
    fn upper_then_lower_round_trips_letters() {
        let input = b"AbCdEfG";
        let mut up = [0u8; 7];
        let mut down = [0u8; 7];
        to_upper_into(input, &mut up);
        to_lower_into(&up, &mut down);
        assert_eq!(&down, b"abcdefg");
    }

    // ============================================================
    // FFI round-trip tests — exercise the extern "C" wrappers
    // through a real Str block alloc → fold → free cycle.
    // ============================================================

    use crate::block::__torajs_str_free;

    fn make_str(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc(payload.len() as u32);
        let dst = unsafe { b.as_bytes_mut(payload.len() as u32) };
        dst.copy_from_slice(payload);
        b.into_raw()
    }

    fn read_payload(p: *const u8) -> Vec<u8> {
        let (payload, _, _) = unsafe { str_view(p) };
        payload.to_vec()
    }

    #[test]
    fn ffi_to_upper_roundtrips() {
        let s = make_str(b"hello, world!");
        let r = unsafe { __torajs_str_to_upper(s) };
        assert_eq!(read_payload(r), b"HELLO, WORLD!");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_to_lower_roundtrips() {
        let s = make_str(b"HELLO, WORLD!");
        let r = unsafe { __torajs_str_to_lower(s) };
        assert_eq!(read_payload(r), b"hello, world!");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }
}
