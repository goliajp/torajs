//! Escape-form parsing — extracted from `runtime_regex.c` L470-681.
//!
//! Handles `\X` after the leading `\` has been consumed by
//! `parse_atom`. Produces either a single `Char` node (literal byte
//! escapes), a `Class` node (shorthand classes `\d`/`\w`/`\s` and
//! their inverses + Unicode property `\p{}`), a `Backref` node (`\N`
//! or `\k<name>`), or a `Concat` of `Char` nodes (multi-byte UTF-8
//! encoding of `\uHHHH` / `\u{HHHH..}`).

use super::{Parser, RE_FLAG_U, apply_property_name, char_node, class_node, hex_value};
use crate::node::{Node, NodeKind};
use crate::utf8::utf8_encode_cp;

impl<'p> Parser<'p> {
    pub(super) fn parse_escape(&mut self) -> Option<Box<Node>> {
        if self.eof() {
            self.set_err();
            return None;
        }
        let c = self.get();
        if (b'1'..=b'9').contains(&c) {
            let mut n = Node::new(NodeKind::Backref);
            n.capture_idx = (c - b'0') as i32;
            return Some(n);
        }
        match c {
            b'n' => Some(char_node(b'\n')),
            b't' => Some(char_node(b'\t')),
            b'r' => Some(char_node(b'\r')),
            b'f' => Some(char_node(0x0C)),
            b'v' => Some(char_node(0x0B)),
            b'0' => Some(char_node(0)),
            b'd' => Some(class_node(|cc| cc.add_digit())),
            b'D' => Some(class_node(|cc| {
                cc.add_digit();
                cc.negate = true;
            })),
            b'w' => Some(class_node(|cc| cc.add_word())),
            b'W' => Some(class_node(|cc| {
                cc.add_word();
                cc.negate = true;
            })),
            b's' => Some(class_node(|cc| cc.add_space())),
            b'S' => Some(class_node(|cc| {
                cc.add_space();
                cc.negate = true;
            })),
            b'b' => Some(Node::new(NodeKind::WBound)),
            b'B' => Some(Node::new(NodeKind::NWBound)),
            b'k' => self.parse_named_backref(),
            b'x' => self.parse_hex_escape(),
            b'u' => self.parse_unicode_escape(),
            b'p' | b'P' => self.parse_property_escape(c),
            other => Some(char_node(other)),
        }
    }

    fn parse_named_backref(&mut self) -> Option<Box<Node>> {
        if self.eof() || self.peek() != b'<' {
            self.set_err();
            return None;
        }
        self.get(); // consume `<`
        let name = self.read_word_name(b'>')?;
        let mut n = Node::new(NodeKind::Backref);
        n.capture_idx = -1; // resolved post-parse
        n.backref_name = name;
        Some(n)
    }

    fn parse_hex_escape(&mut self) -> Option<Box<Node>> {
        let h1 = self.read_hex_digit()?;
        let h2 = self.read_hex_digit()?;
        Some(char_node((h1 << 4) | h2))
    }

    fn parse_unicode_escape(&mut self) -> Option<Box<Node>> {
        let cp = if !self.eof() && self.peek() == b'{' && self.flags & RE_FLAG_U != 0 {
            self.parse_braced_unicode()?
        } else {
            self.parse_4digit_unicode()
        };
        let Some(cp) = cp else {
            // Lenient fallback: bare `\u` not followed by valid form
            // → literal `u` (matches the legacy non-u-mode default).
            return Some(char_node(b'u'));
        };
        let mut buf = [0u8; 4];
        let blen = utf8_encode_cp(cp, &mut buf);
        if blen == 0 {
            self.set_err();
            return None;
        }
        if blen == 1 {
            return Some(char_node(buf[0]));
        }
        let mut seq = Node::new(NodeKind::Concat);
        for &b in &buf[..blen] {
            seq.push_kid(char_node(b));
        }
        Some(seq)
    }

    /// `\u{HHHH..}` extended form (u flag only). Returns `Some(Some(cp))`
    /// on success; `Some(None)` is not used (this form always either
    /// produces a code point or errors). Returns `None` and sets err
    /// on malformed input.
    fn parse_braced_unicode(&mut self) -> Option<Option<i32>> {
        self.get(); // consume `{`
        let mut val: i64 = 0;
        let mut ndig = 0;
        while !self.eof() && self.peek() != b'}' {
            let h = self.get();
            let Some(d) = hex_value(h) else {
                self.set_err();
                return None;
            };
            val = (val << 4) | d as i64;
            if val > 0x10FFFF {
                self.set_err();
                return None;
            }
            ndig += 1;
        }
        if ndig == 0 || self.eof() {
            self.set_err();
            return None;
        }
        self.get(); // consume `}`
        // Lone surrogate is a SyntaxError under u flag per ECMA-262.
        if (0xD800..=0xDFFF).contains(&val) {
            self.set_err();
            return None;
        }
        Some(Some(val as i32))
    }

    /// `\uHHHH` 4-digit form. Returns `Some(cp)` on success; `None`
    /// on a non-hex byte (caller falls back to literal `u`). Does NOT
    /// set err on the fallback path.
    fn parse_4digit_unicode(&mut self) -> Option<i32> {
        if self.remaining() < 4 {
            return None;
        }
        let mut val: i32 = 0;
        for j in 0..4 {
            let d = hex_value(self.byte_at(self.i + j))?;
            val = (val << 4) | d as i32;
        }
        self.i += 4;
        Some(val)
    }

    /// `\p{NAME}` / `\P{NAME}` — Unicode property class. Requires u
    /// flag; without it returns literal `p` / `P`.
    fn parse_property_escape(&mut self, c: u8) -> Option<Box<Node>> {
        if self.flags & RE_FLAG_U == 0 {
            return Some(char_node(c));
        }
        if self.eof() || self.peek() != b'{' {
            self.set_err();
            return None;
        }
        self.get(); // consume `{`
        let name = self.read_word_name(b'}')?;
        let mut n = Node::new(NodeKind::Class);
        let matched = apply_property_name(&mut n, &name);
        if !matched {
            self.set_err();
            return None;
        }
        if c == b'P' {
            n.cc.negate = true;
        }
        Some(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(pattern: &str, flags: u8) -> Box<Node> {
        let mut p = Parser::new(pattern.as_bytes(), flags);
        let r = p.parse().expect("parse failed");
        assert!(!p.err());
        r
    }

    fn parse_err(pattern: &str, flags: u8) {
        let mut p = Parser::new(pattern.as_bytes(), flags);
        let r = p.parse();
        assert!(
            r.is_none() && p.err(),
            "expected parse error for {pattern:?}"
        );
    }

    #[test]
    fn parses_escape_shorthands() {
        for (pat, expect_negate) in [
            ("\\d", false),
            ("\\D", true),
            ("\\w", false),
            ("\\W", true),
            ("\\s", false),
            ("\\S", true),
        ] {
            let r = parse_ok(pat, 0);
            assert_eq!(r.kids[0].kind, NodeKind::Class);
            assert_eq!(r.kids[0].cc.negate, expect_negate);
        }
    }

    #[test]
    fn parses_decimal_backref() {
        let r = parse_ok("(a)\\1", 0);
        let backref = &r.kids[1];
        assert_eq!(backref.kind, NodeKind::Backref);
        assert_eq!(backref.capture_idx, 1);
    }

    #[test]
    fn parses_named_backref() {
        let r = parse_ok("(?<x>a)\\k<x>", 0);
        let backref = &r.kids[1];
        assert_eq!(backref.kind, NodeKind::Backref);
        assert_eq!(backref.capture_idx, -1); // unresolved
        assert_eq!(&backref.backref_name, b"x");
    }

    #[test]
    fn parses_hex_escape() {
        let r = parse_ok("\\x41", 0);
        assert_eq!(r.kids[0].kind, NodeKind::Char);
        assert_eq!(r.kids[0].ch, b'A');
    }

    #[test]
    fn parses_unicode_4digit_escape() {
        let r = parse_ok("\\u0041", 0);
        assert_eq!(r.kids[0].kind, NodeKind::Char);
        assert_eq!(r.kids[0].ch, b'A');
    }

    #[test]
    fn parses_unicode_4digit_multibyte_emits_concat() {
        let r = parse_ok("\\u4E2D", 0); // 中 → 3 UTF-8 bytes
        assert_eq!(r.kids[0].kind, NodeKind::Concat);
        assert_eq!(r.kids[0].kids.len(), 3);
    }

    #[test]
    fn parses_unicode_braced_under_u_flag() {
        let r = parse_ok("\\u{1F600}", RE_FLAG_U); // 😀 → 4 bytes
        assert_eq!(r.kids[0].kind, NodeKind::Concat);
        assert_eq!(r.kids[0].kids.len(), 4);
    }

    #[test]
    fn rejects_lone_surrogate_under_u_flag() {
        parse_err("\\u{D800}", RE_FLAG_U);
    }

    #[test]
    fn parses_unicode_property_letter_under_u_flag() {
        let r = parse_ok("\\p{L}", RE_FLAG_U);
        assert_eq!(r.kids[0].kind, NodeKind::Class);
        assert!(r.kids[0].cc.test_cp(0x03B1)); // α
        assert!(!r.kids[0].cc.negate);
    }

    #[test]
    fn parses_unicode_property_long_name() {
        let r = parse_ok("\\p{Number}", RE_FLAG_U);
        assert_eq!(r.kids[0].kind, NodeKind::Class);
        assert!(r.kids[0].cc.test_cp(0x0664)); // ٤
    }

    #[test]
    fn parses_unicode_property_negated_capital_p() {
        let r = parse_ok("\\P{L}", RE_FLAG_U);
        assert!(r.kids[0].cc.negate);
    }

    #[test]
    fn property_without_u_flag_is_literal() {
        let r = parse_ok("\\p", 0);
        assert_eq!(r.kids[0].kind, NodeKind::Char);
        assert_eq!(r.kids[0].ch, b'p');
    }

    #[test]
    fn parses_word_boundary_escapes() {
        let r = parse_ok("\\b\\B", 0);
        assert_eq!(r.kids[0].kind, NodeKind::WBound);
        assert_eq!(r.kids[1].kind, NodeKind::NWBound);
    }

    #[test]
    fn parses_simple_char_aliases() {
        for (pat, expect) in [
            ("\\n", b'\n'),
            ("\\t", b'\t'),
            ("\\r", b'\r'),
            ("\\f", 0x0C),
            ("\\v", 0x0B),
            ("\\0", 0),
        ] {
            let r = parse_ok(pat, 0);
            assert_eq!(r.kids[0].kind, NodeKind::Char);
            assert_eq!(r.kids[0].ch, expect);
        }
    }

    #[test]
    fn parses_literal_escape_of_metachar() {
        // `\.` → literal `.`, not Any.
        let r = parse_ok("\\.", 0);
        assert_eq!(r.kids[0].kind, NodeKind::Char);
        assert_eq!(r.kids[0].ch, b'.');
    }
}
