//! Generalized UTF-8 decoding: the standard lead-byte shapes, with
//! the surrogate range (`ED A0..BF xx`) admitted instead of
//! rejected. Encoding is the mirror. Both assume well-formed input —
//! the crate's constructors are the only producers.

/// Iterator over the code points of a WTF-8 slice. Surrogates
/// (0xD800..=0xDFFF) yield as themselves.
pub struct CodePoints<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> CodePoints<'a> {
    #[inline]
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        CodePoints { bytes, pos: 0 }
    }
}

impl Iterator for CodePoints<'_> {
    type Item = u32;

    #[inline]
    fn next(&mut self) -> Option<u32> {
        let b = self.bytes;
        let i = self.pos;
        let lead = *b.get(i)?;
        let (cp, w) = if lead < 0x80 {
            (lead as u32, 1)
        } else if lead < 0xE0 {
            (((lead & 0x1F) as u32) << 6 | (b[i + 1] & 0x3F) as u32, 2)
        } else if lead < 0xF0 {
            (
                ((lead & 0x0F) as u32) << 12
                    | ((b[i + 1] & 0x3F) as u32) << 6
                    | (b[i + 2] & 0x3F) as u32,
                3,
            )
        } else {
            (
                ((lead & 0x07) as u32) << 18
                    | ((b[i + 1] & 0x3F) as u32) << 12
                    | ((b[i + 2] & 0x3F) as u32) << 6
                    | (b[i + 3] & 0x3F) as u32,
                4,
            )
        };
        self.pos = i + w;
        Some(cp)
    }
}

/// Encode one code point (surrogates included) as generalized
/// UTF-8. Returns the byte count written into `out`.
#[inline]
pub(crate) fn encode_cp(cp: u32, out: &mut [u8; 4]) -> usize {
    if cp < 0x80 {
        out[0] = cp as u8;
        1
    } else if cp < 0x800 {
        out[0] = 0xC0 | (cp >> 6) as u8;
        out[1] = 0x80 | (cp & 0x3F) as u8;
        2
    } else if cp < 0x10000 {
        out[0] = 0xE0 | (cp >> 12) as u8;
        out[1] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        out[2] = 0x80 | (cp & 0x3F) as u8;
        3
    } else {
        out[0] = 0xF0 | (cp >> 18) as u8;
        out[1] = 0x80 | ((cp >> 12) & 0x3F) as u8;
        out[2] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        out[3] = 0x80 | (cp & 0x3F) as u8;
        4
    }
}

/// Well-formed WTF-8: UTF-8 shapes (no overlongs, ≤ U+10FFFF) with
/// the surrogate 3-byte range admitted, and never a high surrogate
/// 3-byte sequence directly followed by a low one.
pub(crate) fn is_well_formed(b: &[u8]) -> bool {
    let mut i = 0;
    let mut prev_hi = false;
    while i < b.len() {
        let lead = b[i];
        let w = match lead {
            0x00..=0x7F => 1,
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => return false,
        };
        if i + w > b.len() || b[i + 1..i + w].iter().any(|&c| c & 0xC0 != 0x80) {
            return false;
        }
        let b1 = if w > 1 { b[i + 1] } else { 0 };
        let overlong_or_range = match (w, lead) {
            (3, 0xE0) => b1 < 0xA0,
            (4, 0xF0) => b1 < 0x90,
            (4, 0xF4) => b1 > 0x8F,
            _ => false,
        };
        if overlong_or_range {
            return false;
        }
        let hi = w == 3 && lead == 0xED && (0xA0..=0xAF).contains(&b1);
        let lo = w == 3 && lead == 0xED && (0xB0..=0xBF).contains(&b1);
        if prev_hi && lo {
            return false;
        }
        prev_hi = hi;
        i += w;
    }
    true
}

/// UTF-16 code units of one code point: one for the BMP (a
/// surrogate value passes through as itself), two otherwise.
#[inline]
pub(crate) fn cp_to_units(cp: u32) -> impl Iterator<Item = u16> {
    let (a, b) = if cp <= 0xFFFF {
        (cp as u16, None)
    } else {
        let off = cp - 0x10000;
        (
            0xD800 | (off >> 10) as u16,
            Some(0xDC00 | (off & 0x3FF) as u16),
        )
    };
    core::iter::once(a).chain(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn decode_every_width_including_surrogates() {
        let bytes = [
            0x61, 0xC3, 0xA9, 0xED, 0xA0, 0x80, 0xED, 0xBF, 0xBF, 0xF0, 0x9D, 0x92, 0xA2,
        ];
        let cps: Vec<u32> = CodePoints::new(&bytes).collect();
        assert_eq!(cps, [0x61, 0xE9, 0xD800, 0xDFFF, 0x1D4A2]);
    }

    #[test]
    fn encode_round_trips_all_planes() {
        for &cp in &[
            0u32, 0x7F, 0x80, 0x7FF, 0x800, 0xD800, 0xDFFF, 0xFFFF, 0x10000, 0x10FFFF,
        ] {
            let mut buf = [0u8; 4];
            let n = encode_cp(cp, &mut buf);
            let back: Vec<u32> = CodePoints::new(&buf[..n]).collect();
            assert_eq!(back, [cp], "cp {cp:x}");
        }
    }
}
