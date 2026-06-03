//! `s.codePointAt(i)` / `substr.codePointAt(i)` per ES §22.1.3.3.
//!
//! Differs from `charCodeAt`: when the code unit at `i` is a
//! high surrogate (`0xD800..=0xDBFF`) and the next code unit is a
//! low surrogate (`0xDC00..=0xDFFF`), the two combine into the
//! full supplementary-plane codepoint
//! `0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)`. Otherwise
//! the lone code unit is returned as-is (lone high / lone low /
//! BMP non-surrogate / Latin-1 byte).
//!
//! OOB (`i < 0 || i >= length`) returns `0` to mirror the existing
//! `__torajs_str_char_code_at` stub. Spec is `undefined`; the
//! `*_v2` rewrite that surfaces NaN / undefined through the SSA
//! layer is P11.3-A2 follow-up.
//!
//! Latin-1 strings can never carry a surrogate (canonical encoding
//! invariant: Latin-1 covers `0..=0xFF`), so the Latin-1 branch
//! short-circuits to the raw byte.

use crate::lookup::str_view;
use crate::substr_methods::substr_view;

#[inline]
fn combine_or_lone(payload: &[u8], i_cu: usize, length_cu: usize) -> u32 {
    let off = i_cu * 2;
    let lo = payload[off] as u16;
    let hi = payload[off + 1] as u16;
    let unit = ((hi << 8) | lo) as u32;
    if (0xD800..=0xDBFF).contains(&unit) && i_cu + 1 < length_cu {
        let off2 = (i_cu + 1) * 2;
        let lo2 = payload[off2] as u16;
        let hi2 = payload[off2 + 1] as u16;
        let unit2 = ((hi2 << 8) | lo2) as u32;
        if (0xDC00..=0xDFFF).contains(&unit2) {
            return 0x10000 + ((unit - 0xD800) << 10) + (unit2 - 0xDC00);
        }
    }
    unit
}

/// `s.codePointAt(i)` per ES §22.1.3.3. See module docs for the
/// surrogate-pair combine rule.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_code_point_at(s: *const u8, i: i64) -> i64 {
    let (payload, length, is_latin1) = unsafe { str_view(s) };
    if i < 0 || i >= length as i64 {
        return 0;
    }
    if is_latin1 {
        return payload[i as usize] as i64;
    }
    combine_or_lone(payload, i as usize, length as usize) as i64
}

/// `substr.codePointAt(i)` — mirror of the Str path operating on
/// the view's code-unit range. Post-P11.1-S5 the Substr's `len@8`
/// and `offset@24` fields are already JS code units, so the same
/// surrogate-pair combine logic applies.
///
/// # Safety
///
/// `v` is a live `*const Substr` (rc > 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_code_point_at(v: *const u8, i: i64) -> i64 {
    let (payload, cu_len, is_latin1) = unsafe { substr_view(v) };
    if i < 0 || i >= cu_len as i64 {
        return 0;
    }
    if is_latin1 {
        return payload[i as usize] as i64;
    }
    combine_or_lone(payload, i as usize, cu_len) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_or_lone_supplementary() {
        // U+1F600 (😀) — high D83D + low DE00 → 0x1F600
        let payload: [u8; 4] = [0x3D, 0xD8, 0x00, 0xDE];
        assert_eq!(combine_or_lone(&payload, 0, 2), 0x1F600);
    }

    #[test]
    fn combine_or_lone_lone_high_then_bmp() {
        // High D83D followed by 'a' (0x0061 not a low surrogate) → lone high.
        let payload: [u8; 4] = [0x3D, 0xD8, 0x61, 0x00];
        assert_eq!(combine_or_lone(&payload, 0, 2), 0xD83D);
    }

    #[test]
    fn combine_or_lone_lone_low() {
        // Low DE00 standalone (i_cu at low surrogate position) → lone low.
        let payload: [u8; 2] = [0x00, 0xDE];
        assert_eq!(combine_or_lone(&payload, 0, 1), 0xDE00);
    }

    #[test]
    fn combine_or_lone_bmp_non_surrogate() {
        // U+4E2D ("中") — BMP non-surrogate.
        let payload: [u8; 2] = [0x2D, 0x4E];
        assert_eq!(combine_or_lone(&payload, 0, 1), 0x4E2D);
    }

    #[test]
    fn combine_or_lone_high_at_last_index_no_partner() {
        // High D83D at the end (length 1 cu) — no next u16 to combine.
        let payload: [u8; 2] = [0x3D, 0xD8];
        assert_eq!(combine_or_lone(&payload, 0, 1), 0xD83D);
    }
}
