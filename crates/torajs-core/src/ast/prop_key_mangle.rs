//! Property key ↔ symbol spelling (557-02 C 组).
//!
//! A class member's name is a [`PropKey`] — a UTF-16 code-unit
//! sequence, possibly with lone surrogates. The FnDecl / global the
//! desugar mints for it (`__cm_<C>__<m>`, `__sm_<C>__<m>`,
//! `__sf_<C>__<f>`) is a Rust `String`, so the key needs a spelling
//! that fits in one. This is that spelling, and it is a bijection:
//! the runtime key of a method is recovered from its symbol by
//! [`unmangle_key`] exactly, the way `rustc-demangle` recovers a
//! path from a mangled symbol. Two different keys never share a
//! symbol, and no key is ever spelled through U+FFFD.
//!
//! Encoding: a well-formed key that contains no `__u_` is its own
//! spelling (every identifier-shaped name, i.e. all of them in
//! practice, so symbols do not change). Any other key is escaped
//! code point by code point — a lone surrogate becomes `__u_XXXX`
//! (four lowercase hex digits) and every `_` becomes `__u_005f`, so
//! in an escaped spelling a `_` only ever occurs inside an escape
//! and the parse back is unambiguous.

use std::borrow::Cow;

use torajs_wtf8::{Wtf8, Wtf8Buf};

use super::PropKey;

const ESC: &str = "__u_";

/// The symbol spelling of `key` — borrowed when the key is its own
/// spelling, which every identifier-shaped key is.
pub fn mangle_key(key: &Wtf8) -> Cow<'_, str> {
    if let Some(s) = key.as_str()
        && !s.contains(ESC)
    {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(key.len() + 8);
    for cp in key.code_points() {
        if cp == u32::from(b'_') || (0xD800..=0xDFFF).contains(&cp) {
            out.push_str(ESC);
            push_hex4(&mut out, cp);
        } else {
            // Every non-surrogate code point a WTF-8 slice yields is
            // a scalar value.
            out.push(char::from_u32(cp).unwrap());
        }
    }
    Cow::Owned(out)
}

/// The key a symbol spelling stands for — the inverse of
/// [`mangle_key`].
pub fn unmangle_key(sym: &str) -> PropKey {
    if !sym.contains(ESC) {
        return PropKey::from(sym);
    }
    let mut out = Wtf8Buf::with_capacity(sym.len());
    let mut rest = sym;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix(ESC)
            && tail.len() >= 4
            && tail.as_bytes()[..4].iter().all(u8::is_ascii_hexdigit)
            && let Ok(cp) = u32::from_str_radix(&tail[..4], 16)
        {
            out.push_code_point(cp);
            rest = &tail[4..];
            continue;
        }
        let c = rest.chars().next().unwrap();
        out.push_code_point(u32::from(c));
        rest = &rest[c.len_utf8()..];
    }
    PropKey::from(out)
}

fn push_hex4(out: &mut String, cp: u32) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for shift in [12u32, 8, 4, 0] {
        out.push(HEX[((cp >> shift) & 0xF) as usize] as char);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(units: &[u16]) -> PropKey {
        let mut b = Wtf8Buf::new();
        for &u in units {
            b.push_code_point(u32::from(u));
        }
        PropKey::from(b)
    }

    #[test]
    fn identifier_keys_are_their_own_spelling() {
        for s in ["next", "__proto__", "get_x", "0", "中", "😀", "a b"] {
            let k = PropKey::from(s);
            assert!(matches!(mangle_key(&k), Cow::Borrowed(_)));
            assert_eq!(mangle_key(&k), s);
            assert_eq!(unmangle_key(s), k);
        }
    }

    #[test]
    fn lone_surrogates_escape_and_round_trip() {
        let hi = key(&[0xD800]);
        assert_eq!(mangle_key(&hi), "__u_d800");
        assert_eq!(unmangle_key("__u_d800"), hi);
        let lo = key(&[0xDC00]);
        assert_eq!(mangle_key(&lo), "__u_dc00");
        assert_eq!(unmangle_key("__u_dc00"), lo);
        let mixed = key(&[b'a' as u16, b'_' as u16, 0xDFFF, b'z' as u16]);
        assert_eq!(mangle_key(&mixed), "a__u_005f__u_dfffz");
        assert_eq!(unmangle_key("a__u_005f__u_dfffz"), mixed);
    }

    #[test]
    fn a_key_spelling_the_escape_itself_is_escaped() {
        let k = PropKey::from("__u_d800");
        let m = mangle_key(&k);
        assert_ne!(m, "__u_d800");
        assert_eq!(m, "__u_005f__u_005fu__u_005fd800");
        assert_eq!(unmangle_key(&m), k);
        assert_ne!(unmangle_key(&m), key(&[0xD800]));
    }

    #[test]
    fn a_surrogate_pair_is_one_scalar_and_stays_literal() {
        let k = key(&[0xD83D, 0xDE00]);
        assert_eq!(k, PropKey::from("😀"));
        assert_eq!(mangle_key(&k), "😀");
    }

    #[test]
    fn unmangle_is_total_on_arbitrary_symbols() {
        // A `__u_` not followed by four hex digits is literal text.
        assert_eq!(unmangle_key("__u_zz"), PropKey::from("__u_zz"));
        assert_eq!(unmangle_key("__u_12"), PropKey::from("__u_12"));
    }
}
