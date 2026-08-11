//! §19.2.6 URI Handling Functions — the Encode / Decode kernels
//! behind the four globals (`encodeURI` / `encodeURIComponent` /
//! `decodeURI` / `decodeURIComponent`).
//!
//! One kernel per direction, a `component` flag selecting the set:
//! Encode's *unescaped* set is uriUnescaped (alpha / digit / mark)
//! plus — for `encodeURI` only — uriReserved and `#` (§19.2.6.4/.2);
//! Decode's *preserved* set is uriReserved plus `#` for `decodeURI`
//! and empty for `decodeURIComponent` (§19.2.6.1/.3), where a
//! preserved escape keeps its ORIGINAL `%XX` spelling (case and
//! all) rather than re-encoding.
//!
//! The walk is over code units (Latin-1 byte or UTF-16LE pair —
//! `str_view`'s two encodings). Encode assembles code points per
//! §19.2.6.5: a lone surrogate raises URIError, a pair combines
//! before the UTF-8 percent-expansion (uppercase hex). Decode
//! parses `%XX` runs per §19.2.6.6: a truncated or non-hex escape,
//! a bad leading byte (`10xxxxxx` / more than 4 ones), a missing or
//! malformed continuation, and any octet run `core::str::from_utf8`
//! rejects (overlong forms, encoded surrogates, > U+10FFFF) all
//! raise URIError. Supplementary code points re-split into a
//! surrogate pair on output.
//!
//! Both kernels answer a fresh canonical Str
//! ([`crate::alloc_canonical::alloc_units_canonical`] narrows an
//! all-narrow UTF-16 result to Latin-1); the throw path records the
//! pending URIError (torajs-throw slot 6) and answers the empty
//! Str, which the lowerer's `emit_throw_check` discards.

use alloc::vec::Vec;

use crate::block::StrBlock;
use crate::lookup::str_view;

unsafe extern "C" {
    fn __torajs_throw_uri_error(msg: *const core::ffi::c_char);
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// uriUnescaped (§19.2.6): uriAlpha / DecimalDigit / uriMark.
fn is_unescaped_component(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
        )
}

/// uriReserved ∪ `#` (§19.2.6) — extra unescaped set for
/// `encodeURI`, preserved escape set for `decodeURI`.
fn is_reserved_or_hash(b: u8) -> bool {
    matches!(
        b,
        b';' | b'/' | b'?' | b':' | b'@' | b'&' | b'=' | b'+' | b'$' | b',' | b'#'
    )
}

fn hex_val(unit: u32) -> Option<u8> {
    let c = u8::try_from(unit).ok()?;
    (c as char).to_digit(16).map(|d| d as u8)
}

/// The code unit at `i` under either encoding.
fn unit_at(payload: &[u8], is_latin1: bool, i: usize) -> u32 {
    if is_latin1 {
        payload[i] as u32
    } else {
        u16::from_le_bytes([payload[i * 2], payload[i * 2 + 1]]) as u32
    }
}

/// §19.2.6.5 Encode — `Err` = URIError (lone surrogate).
fn encode_units(
    payload: &[u8],
    len: usize,
    is_latin1: bool,
    component: bool,
) -> Result<Vec<u8>, ()> {
    let mut out: Vec<u8> = Vec::with_capacity(len);
    let mut i = 0usize;
    while i < len {
        let unit = unit_at(payload, is_latin1, i);
        if unit < 0x80 {
            let b = unit as u8;
            if is_unescaped_component(b) || (!component && is_reserved_or_hash(b)) {
                out.push(b);
                i += 1;
                continue;
            }
        }
        // Code-point assembly (step 4.d): a high surrogate must pair
        // with the next unit; a lone half in either order raises.
        let cp = if (0xD800..=0xDBFF).contains(&unit) {
            if i + 1 >= len {
                return Err(());
            }
            let lo = unit_at(payload, is_latin1, i + 1);
            if !(0xDC00..=0xDFFF).contains(&lo) {
                return Err(());
            }
            i += 2;
            0x10000 + ((unit - 0xD800) << 10) + (lo - 0xDC00)
        } else if (0xDC00..=0xDFFF).contains(&unit) {
            return Err(());
        } else {
            i += 1;
            unit
        };
        let mut buf = [0u8; 4];
        let enc = char::from_u32(cp).ok_or(())?.encode_utf8(&mut buf);
        for &b in enc.as_bytes() {
            out.push(b'%');
            out.push(HEX_UPPER[(b >> 4) as usize]);
            out.push(HEX_UPPER[(b & 0xf) as usize]);
        }
    }
    Ok(out)
}

/// §19.2.6.6 Decode — `Err` = URIError (malformed escape / bad
/// UTF-8 octet run). Output is UTF-16 code units.
fn decode_units(
    payload: &[u8],
    len: usize,
    is_latin1: bool,
    component: bool,
) -> Result<Vec<u16>, ()> {
    let mut out: Vec<u16> = Vec::with_capacity(len);
    let mut i = 0usize;
    while i < len {
        let unit = unit_at(payload, is_latin1, i);
        if unit != u32::from(b'%') {
            out.push(unit as u16);
            i += 1;
            continue;
        }
        if i + 2 >= len {
            return Err(());
        }
        let h1 = hex_val(unit_at(payload, is_latin1, i + 1)).ok_or(())?;
        let h2 = hex_val(unit_at(payload, is_latin1, i + 2)).ok_or(())?;
        let b0 = (h1 << 4) | h2;
        if b0 < 0x80 {
            // Single-octet: decodeURI keeps a reserved character's
            // ORIGINAL escape text (case preserved) — step 4.d.vii.
            if !component && is_reserved_or_hash(b0) {
                out.push(b'%' as u16);
                out.push(unit_at(payload, is_latin1, i + 1) as u16);
                out.push(unit_at(payload, is_latin1, i + 2) as u16);
            } else {
                out.push(b0 as u16);
            }
            i += 3;
            continue;
        }
        // Multi-octet run: leading-ones arity, then each
        // continuation must be an escaped 10xxxxxx octet.
        let n = b0.leading_ones() as usize;
        if n < 2 || n > 4 {
            return Err(());
        }
        let mut octets = [0u8; 4];
        octets[0] = b0;
        i += 3;
        for slot in octets.iter_mut().take(n).skip(1) {
            if i + 2 >= len || unit_at(payload, is_latin1, i) != u32::from(b'%') {
                return Err(());
            }
            let h1 = hex_val(unit_at(payload, is_latin1, i + 1)).ok_or(())?;
            let h2 = hex_val(unit_at(payload, is_latin1, i + 2)).ok_or(())?;
            let b = (h1 << 4) | h2;
            if b & 0xC0 != 0x80 {
                return Err(());
            }
            *slot = b;
            i += 3;
        }
        // Strict UTF-8 judgement — from_utf8 rejects overlong
        // forms, encoded surrogates and > U+10FFFF.
        let s = core::str::from_utf8(&octets[..n]).map_err(|_| ())?;
        let mut chars = s.chars();
        let c = chars.next().ok_or(())?;
        if chars.next().is_some() {
            return Err(());
        }
        let cp = c as u32;
        if cp <= 0xFFFF {
            out.push(cp as u16);
        } else {
            let v = cp - 0x10000;
            out.push((0xD800 + (v >> 10)) as u16);
            out.push((0xDC00 + (v & 0x3FF)) as u16);
        }
    }
    Ok(out)
}

fn throw_and_empty(msg: &'static core::ffi::CStr) -> *mut u8 {
    // SAFETY: msg is a static NUL-terminated string.
    unsafe { __torajs_throw_uri_error(msg.as_ptr()) };
    StrBlock::alloc(0).into_raw()
}

/// `encodeURI(s)` (component = 0) / `encodeURIComponent(s)`
/// (component = 1) — §19.2.6.4 / §19.2.6.2.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_uri_encode(s: *const u8, component: i64) -> *mut u8 {
    let (payload, len, is_latin1) = unsafe { str_view(s) };
    match encode_units(payload, len as usize, is_latin1, component != 0) {
        Ok(bytes) => crate::alloc_canonical::alloc_units_canonical(&bytes, true),
        Err(()) => throw_and_empty(c"URI malformed"),
    }
}

/// `decodeURI(s)` (component = 0) / `decodeURIComponent(s)`
/// (component = 1) — §19.2.6.1 / §19.2.6.3.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_uri_decode(s: *const u8, component: i64) -> *mut u8 {
    let (payload, len, is_latin1) = unsafe { str_view(s) };
    match decode_units(payload, len as usize, is_latin1, component != 0) {
        Ok(units) => {
            let mut bytes: Vec<u8> = Vec::with_capacity(units.len() * 2);
            for u in &units {
                bytes.extend_from_slice(&u.to_le_bytes());
            }
            crate::alloc_canonical::alloc_units_canonical(&bytes, false)
        }
        Err(()) => throw_and_empty(c"URI malformed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(s: &str, component: bool) -> Result<Vec<u8>, ()> {
        let units: Vec<u16> = s.encode_utf16().collect();
        let mut bytes = Vec::new();
        for u in &units {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        encode_units(&bytes, units.len(), false, component)
    }

    fn dec(s: &str, component: bool) -> Result<alloc::string::String, ()> {
        let units: Vec<u16> = s.encode_utf16().collect();
        let mut bytes = Vec::new();
        for u in &units {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let out = decode_units(&bytes, units.len(), false, component)?;
        Ok(alloc::string::String::from_utf16(&out).unwrap())
    }

    #[test]
    fn encode_passthrough_and_reserved() {
        assert_eq!(enc("abc123-_.!~*'()", true).unwrap(), b"abc123-_.!~*'()");
        // uriReserved + # pass through encodeURI, escape in component
        assert_eq!(enc(";/?:@&=+$,#", false).unwrap(), b";/?:@&=+$,#");
        assert_eq!(
            enc(";", true).unwrap(),
            b"%3B",
            "component escapes reserved"
        );
        assert_eq!(enc(" ", false).unwrap(), b"%20");
    }

    #[test]
    fn encode_multibyte_and_surrogates() {
        // U+00E9 é → C3 A9; U+4E16 世 → E4 B8 96; U+1F600 😀 → F0 9F 98 80
        assert_eq!(enc("é", false).unwrap(), b"%C3%A9");
        assert_eq!(enc("世", false).unwrap(), b"%E4%B8%96");
        assert_eq!(enc("😀", false).unwrap(), b"%F0%9F%98%80");
        // lone high surrogate raises
        let lone: Vec<u16> = alloc::vec![0xD800];
        let mut bytes = Vec::new();
        for u in &lone {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        assert!(encode_units(&bytes, 1, false, false).is_err());
    }

    #[test]
    fn decode_roundtrip_and_preserve() {
        assert_eq!(dec("%C3%A9", true).unwrap(), "é");
        assert_eq!(dec("%F0%9F%98%80", true).unwrap(), "😀");
        // decodeURI preserves reserved escapes with original case
        assert_eq!(dec("%2f%3B", false).unwrap(), "%2f%3B");
        // decodeURIComponent decodes them
        assert_eq!(dec("%2f%3B", true).unwrap(), "/;");
        assert_eq!(dec("%41", false).unwrap(), "A");
    }

    #[test]
    fn decode_malformed_raises() {
        for bad in [
            "%",
            "%A",
            "%G1",
            "%1G",             // truncated / non-hex
            "%C3",             // missing continuation
            "%C3%C3",          // bad continuation
            "%C0%80",          // overlong NUL
            "%ED%A0%80",       // encoded surrogate
            "%F8%80%80%80%80", // 5-byte arity
            "%80",             // bare continuation
        ] {
            assert!(dec(bad, true).is_err(), "{bad} should raise");
        }
    }
}
