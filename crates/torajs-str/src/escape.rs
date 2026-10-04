//! §B.2.1 `escape` / `unescape` — the two Annex B string globals.
//!
//! Both walk CODE UNITS, not code points: a surrogate pair is two
//! independent `%uXXXX` escapes on the way out and comes back as the
//! same two units, which is why `escape` cannot be written in terms of
//! the URI encoder next door (that one combines pairs into UTF-8 and
//! raises URIError on a lone surrogate — Annex B does neither).
//!
//! `escape` keeps the set §B.2.1.1 names — ASCII alphanumerics plus
//! `@*_+-./` — spells a unit below 256 as `%XX` and anything above as
//! `%uXXXX`, hex uppercase. The result is pure ASCII by construction.
//!
//! `unescape` (§B.2.1.2) is the inverse and never fails: a `%` that is
//! not followed by `uXXXX` or `XX` with hex digits is an ordinary
//! character, kept as written. `%u` wins over `%X` when both could
//! match, and the scan continues after whatever it consumed.

use alloc::vec::Vec;

use crate::lookup::str_view;

/// The unit at `i` in either of `str_view`'s two encodings.
fn unit_at(payload: &[u8], i: usize, is_latin1: bool) -> u16 {
    if is_latin1 {
        payload[i] as u16
    } else {
        u16::from_le_bytes([payload[i * 2], payload[i * 2 + 1]])
    }
}

/// §B.2.1.1 step 4.b — `A-Za-z0-9@*_+-./` are written through.
fn is_unescaped(u: u16) -> bool {
    matches!(u,
        0x41..=0x5A | 0x61..=0x7A | 0x30..=0x39
        | 0x40 | 0x2A | 0x5F | 0x2B | 0x2D | 0x2E | 0x2F)
}

fn hex_digit(n: u16) -> u8 {
    let n = n as u8 & 0xF;
    if n < 10 { b'0' + n } else { b'A' + (n - 10) }
}

/// `Some(value)` when the unit is an ASCII hex digit.
fn hex_val(u: u16) -> Option<u16> {
    match u {
        0x30..=0x39 => Some(u - 0x30),
        0x41..=0x46 => Some(u - 0x41 + 10),
        0x61..=0x66 => Some(u - 0x61 + 10),
        _ => None,
    }
}

fn escape_units(payload: &[u8], len: usize, is_latin1: bool) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(len);
    for i in 0..len {
        let u = unit_at(payload, i, is_latin1);
        if is_unescaped(u) {
            out.push(u as u8);
        } else if u < 256 {
            out.push(b'%');
            out.push(hex_digit(u >> 4));
            out.push(hex_digit(u));
        } else {
            out.push(b'%');
            out.push(b'u');
            out.push(hex_digit(u >> 12));
            out.push(hex_digit(u >> 8));
            out.push(hex_digit(u >> 4));
            out.push(hex_digit(u));
        }
    }
    out
}

fn unescape_units(payload: &[u8], len: usize, is_latin1: bool) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::with_capacity(len);
    let mut i = 0usize;
    while i < len {
        let u = unit_at(payload, i, is_latin1);
        if u != 0x25 {
            out.push(u);
            i += 1;
            continue;
        }
        // `%uXXXX` first — §B.2.1.2 step 4.d tries the six-unit form
        // before the three-unit one.
        if i + 5 < len && unit_at(payload, i + 1, is_latin1) == 0x75 {
            let mut v: u16 = 0;
            let mut ok = true;
            for k in 2..6 {
                match hex_val(unit_at(payload, i + k, is_latin1)) {
                    Some(d) => v = (v << 4) | d,
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                out.push(v);
                i += 6;
                continue;
            }
        }
        if i + 2 < len {
            let hi = hex_val(unit_at(payload, i + 1, is_latin1));
            let lo = hex_val(unit_at(payload, i + 2, is_latin1));
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(u);
        i += 1;
    }
    out
}

/// `escape(s)` — §B.2.1.1.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_escape(s: *const u8) -> *mut u8 {
    let (payload, len, is_latin1) = unsafe { str_view(s) };
    let bytes = escape_units(payload, len as usize, is_latin1);
    crate::alloc_canonical::alloc_built_units(&bytes, true)
}

/// `unescape(s)` — §B.2.1.2.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_unescape(s: *const u8) -> *mut u8 {
    let (payload, len, is_latin1) = unsafe { str_view(s) };
    let units = unescape_units(payload, len as usize, is_latin1);
    let mut bytes: Vec<u8> = Vec::with_capacity(units.len() * 2);
    for u in &units {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    crate::alloc_canonical::alloc_built_units(&bytes, false)
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    fn esc(s: &str) -> alloc::string::String {
        let units: Vec<u16> = s.encode_utf16().collect();
        let mut bytes = alloc::vec::Vec::new();
        for u in &units {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        alloc::string::String::from_utf8(escape_units(&bytes, units.len(), false)).unwrap()
    }

    fn unesc(s: &str) -> alloc::vec::Vec<u16> {
        let units: Vec<u16> = s.encode_utf16().collect();
        let mut bytes = alloc::vec::Vec::new();
        for u in &units {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        unescape_units(&bytes, units.len(), false)
    }

    #[test]
    fn escape_keeps_the_unescaped_set() {
        assert_eq!(esc("abcXYZ098@*_+-./"), "abcXYZ098@*_+-./");
    }

    #[test]
    fn escape_spells_latin1_and_above() {
        assert_eq!(esc(" "), "%20");
        assert_eq!(esc("~"), "%7E");
        assert_eq!(esc("\u{00FF}"), "%FF");
        assert_eq!(esc("\u{0100}"), "%u0100");
        assert_eq!(esc("\u{4E2D}"), "%u4E2D");
    }

    #[test]
    fn escape_walks_units_not_code_points() {
        // U+1D306 is a surrogate pair: two independent escapes.
        assert_eq!(esc("\u{1D306}"), "%uD834%uDF06");
    }

    #[test]
    fn unescape_inverts_both_forms() {
        assert_eq!(unesc("%20"), vec![0x20]);
        assert_eq!(unesc("%u4E2D"), vec![0x4E2D]);
        assert_eq!(unesc("%FF"), vec![0xFF]);
        assert_eq!(unesc("%uD834%uDF06"), vec![0xD834, 0xDF06]);
    }

    #[test]
    fn unescape_keeps_what_it_cannot_parse() {
        assert_eq!(unesc("%"), vec![0x25]);
        assert_eq!(unesc("%2"), vec![0x25, 0x32]);
        assert_eq!(unesc("%zz"), vec![0x25, 0x7A, 0x7A]);
        assert_eq!(unesc("%u12"), vec![0x25, 0x75, 0x31, 0x32]);
        assert_eq!(unesc("%u123z"), vec![0x25, 0x75, 0x31, 0x32, 0x33, 0x7A]);
        // Lowercase hex is accepted on the way in.
        assert_eq!(unesc("%ff"), vec![0xFF]);
    }
}
