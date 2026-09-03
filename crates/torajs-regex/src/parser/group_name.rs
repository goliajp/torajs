//! §22.2.1 `RegExpIdentifierName` — the body of a `(?<name>` specifier
//! and of a `\k<name>` reference.
//!
//! It is an identifier, not a run of word bytes. The first code point
//! must be `IdentifierStartChar`, the rest `IdentifierPartChar`, and
//! either may be spelled `\uHHHH` / `\u{H…H}` instead of written
//! directly. Outside u/v mode the pattern is a sequence of code units,
//! so a supplementary character arrives as a surrogate pair — in the
//! escape spelling it is a pair in *every* mode. Both halves are read
//! as units and folded here, which is what lets a name defined one way
//! match a reference written the other and gives `.groups` the key the
//! source wrote.
//!
//! Every rejection below is a §22.2.1.1 Early Error, so it belongs at
//! parse time: the surrounding literal never compiles.

use super::Parser;
use crate::ucd::{is_identifier_part_cp, is_identifier_start_cp};
use crate::utf8::{utf8_decode_cp, utf8_encode_cp, utf8_len_for};
use alloc::vec::Vec;

const LEAD: core::ops::RangeInclusive<u32> = 0xD800..=0xDBFF;
const TRAIL: core::ops::RangeInclusive<u32> = 0xDC00..=0xDFFF;

impl Parser<'_> {
    /// Read a `RegExpIdentifierName` up to its closing `>`, which is
    /// consumed. Returns the name as UTF-8 with escapes resolved and
    /// surrogate pairs merged; `None` (setting `err`) on EOF, an empty
    /// name, or a code point the production does not allow.
    pub(super) fn read_group_name(&mut self) -> Option<Vec<u8>> {
        // Units first, then judge. A supplementary character is two
        // units in both spellings and neither half is on its own an
        // IdentifierStart or -Part, so a per-unit test would reject
        // every astral name.
        let mut units: Vec<u32> = Vec::new();
        loop {
            if self.eof() {
                self.err = true;
                return None;
            }
            if self.peek() == b'>' {
                self.get();
                break;
            }
            units.push(self.read_group_name_unit()?);
        }
        let mut name: Vec<u8> = Vec::new();
        let mut k = 0;
        while k < units.len() {
            let mut cp = units[k];
            k += 1;
            if LEAD.contains(&cp)
                && let Some(&lo) = units.get(k)
                && TRAIL.contains(&lo)
            {
                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                k += 1;
            }
            let ok = if name.is_empty() {
                is_identifier_start_cp(cp)
            } else {
                is_identifier_part_cp(cp)
            };
            if !ok {
                self.err = true;
                return None;
            }
            let mut buf = [0u8; 4];
            let n = utf8_encode_cp(cp as i32, &mut buf);
            name.extend_from_slice(&buf[..n]);
        }
        if name.is_empty() {
            self.err = true;
            return None;
        }
        Some(name)
    }

    /// One unit of the name: a `\u` escape, or the code point written
    /// directly. A lone surrogate comes back as itself and fails the
    /// identifier test above unless its partner follows.
    fn read_group_name_unit(&mut self) -> Option<u32> {
        if self.peek() == b'\\' {
            self.get();
            if self.eof() || self.get() != b'u' {
                self.err = true;
                return None;
            }
            // The braced form is `[+UnicodeMode]` in the grammar for
            // patterns, but `RegExpIdentifierStart` / `-Part` spell
            // their escape `RegExpUnicodeEscapeSequence[+UnicodeMode]`
            // unconditionally: a name may reach an astral character
            // that way even in a pattern with no `u` flag.
            let cp = if !self.eof() && self.peek() == b'{' {
                self.parse_braced_unicode()??
            } else {
                match self.parse_4digit_unicode() {
                    Some(v) => v,
                    None => {
                        self.err = true;
                        return None;
                    }
                }
            };
            return Some(cp as u32);
        }
        let rest = &self.p[self.i..];
        if rest.len() < utf8_len_for(rest[0]) {
            self.err = true;
            return None;
        }
        let (cp, w) = utf8_decode_cp(rest);
        self.i += w;
        Some(cp as u32)
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::Parser;

    fn names(pattern: &str, flags: u8) -> Option<alloc::vec::Vec<alloc::vec::Vec<u8>>> {
        let mut p = Parser::new(pattern.as_bytes(), flags);
        p.parse()?;
        Some(p.names.clone())
    }

    fn ok(pattern: &str) -> bool {
        names(pattern, 0).is_some()
    }

    #[test]
    fn dollar_and_underscore_start_a_name() {
        assert!(ok("(?<$x>a)"));
        assert!(ok("(?<_x>a)"));
        assert!(ok("(?<x$>a)"));
    }

    #[test]
    fn a_digit_may_not_start_one() {
        assert!(!ok("(?<42a>a)"));
        assert!(!ok("(?<0>a)"));
        assert!(ok("(?<a42>a)"));
    }

    #[test]
    fn unicode_id_start_is_a_name() {
        assert!(ok("(?<日>a)"));
        assert!(ok("(?<αβ>a)"));
        // U+2022 BULLET is neither ID_Start nor ID_Continue.
        assert!(!ok("(?<\u{2022}>a)"));
    }

    #[test]
    fn escape_spelling_resolves_to_the_same_name() {
        let plain = names("(?<foo>a)", 0).unwrap();
        let escaped = names("(?<\\u0066oo>a)", 0).unwrap();
        assert_eq!(plain[1], escaped[1]);
        assert_eq!(plain[1], b"foo");
    }

    #[test]
    fn a_surrogate_pair_is_one_identifier_character() {
        // U+1D453 MATHEMATICAL ITALIC SMALL F is ID_Start; each half
        // of its pair is not.
        assert!(ok("(?<\\u{1d453}>a)"));
        assert!(ok("(?<\\ud835\\udc53>a)"));
        assert!(!ok("(?<\\ud835>a)"));
        let braced = names("(?<\\u{1d453}>a)", 0).unwrap();
        let paired = names("(?<\\ud835\\udc53>a)", 0).unwrap();
        assert_eq!(braced[1], paired[1]);
        assert_eq!(braced[1], "\u{1d453}".as_bytes());
    }

    #[test]
    fn a_reference_matches_a_name_spelled_the_other_way() {
        // Whether a `\k<name>` has a group is decided by
        // `resolve_backrefs`, not by the walk — the name table is only
        // complete once the pattern ends.
        let resolves = |pattern: &str| {
            let mut p = Parser::new(pattern.as_bytes(), 0);
            let Some(mut root) = p.parse() else {
                return false;
            };
            crate::resolve::resolve_backrefs(&mut root, &p.names, p.n_captures, 0)
        };
        assert!(resolves("(?<\\u{1d453}>a)\\k<\u{1d453}>"));
        assert!(!resolves("(?<\\u{1d453}>a)\\k<b>"));
    }
}
