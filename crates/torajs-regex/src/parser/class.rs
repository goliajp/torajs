//! Char-class `[...]` parsing — extracted from `runtime_regex.c`
//! L683-822.
//!
//! Builds a single [`Class`](NodeKind::Class) node whose
//! [`CharClass`] state is populated by:
//! - literal bytes,
//! - byte ranges `c-c2`,
//! - escapes inside the class (`\n`/`\t`/`\xHH`/`\d`/`\w`/`\s`/`\p{}`),
//! - the leading `^` for negation,
//! - the special empty form `[]` / `[^]`.

use super::{Parser, apply_property_name, unicode_mode};
use crate::node::{Node, NodeKind};
use alloc::boxed::Box;

impl<'p> Parser<'p> {
    pub(super) fn parse_class(&mut self) -> Option<Box<Node>> {
        let mut n = Node::new(NodeKind::Class);
        if !self.eof() && self.peek() == b'^' {
            n.cc.negate = true;
            self.get();
        }
        // Empty `[]` is a valid class that matches nothing; `[^]`
        // matches anything. Detect the empty form before the loop
        // body would consume `]` as a literal.
        if !self.eof() && self.peek() == b']' {
            self.get();
            return Some(n);
        }
        loop {
            if self.eof() {
                self.set_err();
                return None;
            }
            if self.peek() == b']' {
                break;
            }
            // First-char (post-`\`) of the range, OR a continue
            // marker when the escape was a shorthand class (`\d`, …)
            // that was applied directly to the class.
            let first = match self.parse_class_item(&mut n)? {
                ClassItem::Char(c) => c,
                ClassItem::ContinueLoop => {
                    // Shorthand class-escape (`\d` / `\p{}` / …) was
                    // just applied to `cc`. In u/v mode it cannot be
                    // a range endpoint per §22.2.1.1 NonemptyClassRanges
                    // Early Errors — a `-` following it (and not
                    // closing with `]`) tries to open a range, reject.
                    // Non-u annexB path lets `-` fall through as a
                    // literal on the next iteration.
                    if unicode_mode(self.flags)
                        && !self.eof()
                        && self.peek() == b'-'
                        && self.i + 1 < self.p.len()
                        && self.byte_at(self.i + 1) != b']'
                    {
                        self.set_err();
                        return None;
                    }
                    continue;
                }
            };
            // Optional range `c-c2`. Bun matches the C port's lookahead:
            // a `-` followed by `]` is a literal hyphen, not a range
            // intro.
            if !self.eof()
                && self.peek() == b'-'
                && self.i + 1 < self.p.len()
                && self.byte_at(self.i + 1) != b']'
            {
                self.get(); // consume `-`
                let hi = self.parse_class_range_end()?;
                // §22.2.1.1 CharacterClassRange Early Error: `[z-a]`
                // is a SyntaxError in every mode (u, v, and non-u).
                if first > hi {
                    self.set_err();
                    return None;
                }
                n.cc.add_range(first, hi);
            } else {
                n.cc.add(first);
            }
        }
        self.get(); // consume `]`
        Some(n)
    }

    /// Parse the next item inside `[...]`. Returns `ClassItem::Char`
    /// for a literal byte, `ClassItem::ContinueLoop` when the item
    /// was a shorthand-class escape (`\d`, `\D`, `\w`, `\W`, `\s`,
    /// `\S`, `\p{}`) that was already applied to `cc`.
    fn parse_class_item(&mut self, n: &mut Node) -> Option<ClassItem> {
        if self.peek() == b'\\' {
            self.get();
            if self.eof() {
                self.set_err();
                return None;
            }
            let e = self.get();
            match e {
                b'd' => {
                    n.cc.add_digit();
                    return Some(ClassItem::ContinueLoop);
                }
                b'D' => {
                    add_complement_digit(n);
                    return Some(ClassItem::ContinueLoop);
                }
                b'w' => {
                    n.cc.add_word();
                    return Some(ClassItem::ContinueLoop);
                }
                b'W' => {
                    add_complement_word(n);
                    return Some(ClassItem::ContinueLoop);
                }
                b's' => {
                    n.cc.add_space();
                    return Some(ClassItem::ContinueLoop);
                }
                b'S' => {
                    add_complement_space(n);
                    return Some(ClassItem::ContinueLoop);
                }
                b'p' => return self.parse_class_property(n),
                _ => {}
            }
            let c = self.read_class_escape_char(e)?;
            Some(ClassItem::Char(c))
        } else {
            Some(ClassItem::Char(self.get()))
        }
    }

    /// Shared escape-char reader for both class items and class-range
    /// endpoints. `e` is the byte immediately after `\`. Returns the
    /// literal byte value (or `None` + `set_err` on malformed escape).
    /// Handles `\n \t \r \f \v \0 \b` literals, `\xHH` hex, and — under
    /// non-u/v mode — annexB §B.1.4 LegacyOctalEscapeSequence (`\N` /
    /// `\NN` / `\NNN`, first digit 0-3 up to 3 octal digits, 4-7 up to
    /// 2). Anything else falls through as literal byte `e` (annexB
    /// IdentityEscape).
    fn read_class_escape_char(&mut self, e: u8) -> Option<u8> {
        match e {
            b'n' => Some(b'\n'),
            b't' => Some(b'\t'),
            b'r' => Some(b'\r'),
            b'f' => Some(0x0C),
            b'v' => Some(0x0B),
            b'b' => Some(0x08),
            b'0' => Some(0),
            b'x' => {
                let h1 = self.read_hex_digit()?;
                let h2 = self.read_hex_digit()?;
                Some((h1 << 4) | h2)
            }
            b'1'..=b'7' if !unicode_mode(self.flags) => {
                let mut n: u32 = (e - b'0') as u32;
                let max_more = if e <= b'3' { 2 } else { 1 };
                let mut digits_read = 0;
                while digits_read < max_more && !self.eof() {
                    let c = self.peek();
                    if (b'0'..=b'7').contains(&c) {
                        self.get();
                        n = n * 8 + (c - b'0') as u32;
                        digits_read += 1;
                    } else {
                        break;
                    }
                }
                Some((n & 0xff) as u8)
            }
            other => Some(other),
        }
    }

    /// `\p{}` inside `[...]` under the u flag. Without the u flag
    /// returns literal `p`. `\P` complement inside class is L3b.
    fn parse_class_property(&mut self, n: &mut Node) -> Option<ClassItem> {
        if !unicode_mode(self.flags) {
            return Some(ClassItem::Char(b'p'));
        }
        if self.eof() || self.peek() != b'{' {
            self.set_err();
            return None;
        }
        self.get(); // consume `{`
        let (name, value) = self.read_property_expr()?;
        let matched = apply_property_name(n, &name, value.as_deref());
        if !matched {
            self.set_err();
            return None;
        }
        Some(ClassItem::ContinueLoop)
    }

    /// Parse the high end of a `c-hi` class range (just-consumed `-`).
    /// Delegates escape decoding to `read_class_escape_char` so `\xHH`
    /// hex and annexB `\NN` legacy octal escapes work as range
    /// endpoints (bun-accept, previously tr rejected → misfired
    /// `SyntaxError` at `ast_desugar_regex_syntax_error`).
    fn parse_class_range_end(&mut self) -> Option<u8> {
        if self.peek() == b'\\' {
            self.get();
            if self.eof() {
                self.set_err();
                return None;
            }
            let e = self.get();
            // §22.2.1.1 NonemptyClassRanges Early Error under u/v:
            // a shorthand class-escape can't be a range endpoint.
            // Non-u annexB path falls through as a literal char.
            if unicode_mode(self.flags)
                && matches!(e, b'd' | b'D' | b'w' | b'W' | b's' | b'S' | b'p' | b'P')
            {
                self.set_err();
                return None;
            }
            self.read_class_escape_char(e)
        } else {
            Some(self.get())
        }
    }
}

enum ClassItem {
    Char(u8),
    ContinueLoop,
}

fn add_complement_digit(n: &mut Node) {
    for k in 0..=255u32 {
        let k = k as u8;
        if !k.is_ascii_digit() {
            n.cc.add(k);
        }
    }
}

fn add_complement_word(n: &mut Node) {
    for k in 0..=255u32 {
        let k = k as u8;
        let is_w = k.is_ascii_alphanumeric() || k == b'_';
        if !is_w {
            n.cc.add(k);
        }
    }
}

fn add_complement_space(n: &mut Node) {
    for k in 0..=255u32 {
        let k = k as u8;
        let is_s = matches!(k, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r');
        if !is_s {
            n.cc.add(k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::RE_FLAG_U;

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
    fn parses_char_class_simple() {
        let r = parse_ok("[abc]", 0);
        let class = &r.kids[0];
        assert_eq!(class.kind, NodeKind::Class);
        for c in [b'a', b'b', b'c'] {
            assert!(class.cc.test(c));
        }
        assert!(!class.cc.test(b'd'));
    }

    #[test]
    fn parses_char_class_range() {
        let r = parse_ok("[a-z]", 0);
        let class = &r.kids[0];
        for c in b'a'..=b'z' {
            assert!(class.cc.test(c));
        }
    }

    #[test]
    fn parses_char_class_negated() {
        let r = parse_ok("[^abc]", 0);
        let class = &r.kids[0];
        assert!(class.cc.negate);
    }

    #[test]
    fn parses_empty_char_class() {
        let r = parse_ok("[]", 0);
        assert_eq!(r.kids[0].kind, NodeKind::Class);
        assert!(!r.kids[0].cc.negate);
    }

    #[test]
    fn parses_negated_empty_char_class_matches_anything() {
        let r = parse_ok("[^]", 0);
        let class = &r.kids[0];
        assert!(class.cc.negate);
    }

    #[test]
    fn parses_class_with_shorthand_escape() {
        let r = parse_ok("[a\\d]", 0);
        let class = &r.kids[0];
        assert!(class.cc.test(b'a'));
        assert!(class.cc.test(b'0'));
    }

    #[test]
    fn parses_class_with_property_under_u_flag() {
        let r = parse_ok("[\\p{L}_]", RE_FLAG_U);
        let class = &r.kids[0];
        assert!(class.cc.test(b'_'));
        assert!(class.cc.test_cp(0x03B1)); // α
    }

    /// RFC 20260711 chunk B — keyed `Name=Value` forms work inside
    /// `[...]` through the shared `read_property_expr` /
    /// `apply_property_name` pair; two property escapes union.
    #[test]
    fn parses_class_with_property_name_value() {
        let r = parse_ok("[\\p{Script=Greek}x]", RE_FLAG_U);
        let class = &r.kids[0];
        assert!(class.cc.test(b'x'));
        assert!(class.cc.test_cp(0x03B1)); // α — Script=Greek
        assert!(!class.cc.test_cp(0x0451)); // ё — Cyrillic

        let r = parse_ok("[\\p{Lu}\\p{Nd}]", RE_FLAG_U);
        let class = &r.kids[0];
        assert!(class.cc.test_cp(0x0391)); // Α — Lu
        assert!(class.cc.test_cp(0x0664)); // ٤ — Nd
        assert!(!class.cc.test_cp(0x03B1)); // α is Ll

        parse_err("[\\p{Foo}]", RE_FLAG_U);
        parse_err("[\\p{Script=NotAScript}]", RE_FLAG_U);
    }

    #[test]
    fn parses_class_with_hex_escape() {
        let r = parse_ok("[\\x41]", 0);
        let class = &r.kids[0];
        assert!(class.cc.test(b'A'));
    }

    #[test]
    fn hyphen_before_close_bracket_is_literal() {
        let r = parse_ok("[a-]", 0);
        let class = &r.kids[0];
        assert!(class.cc.test(b'a'));
        assert!(class.cc.test(b'-'));
        assert!(!class.cc.test(b'b'));
    }

    #[test]
    fn complement_shorthand_inside_class() {
        let r = parse_ok("[\\D]", 0);
        let class = &r.kids[0];
        assert!(class.cc.test(b'A'));
        assert!(!class.cc.test(b'0'));
    }
}
