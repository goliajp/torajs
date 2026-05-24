//! Regex pattern parser — port of `runtime_regex.c` L431-1029.
//!
//! Recursive descent over pattern bytes; mutually recursive grammar
//! `alt → concat → repeat → atom → alt`. Produces an AST of
//! [`crate::node::Node`] which the future compiler (P6.2-c) turns
//! into Thompson NFA bytecode.
//!
//! Error semantics mirror the C port: any malformed input sets
//! `self.err = true` and returns `None`. Rust's `Drop` recursively
//! frees in-progress sub-trees on the `None` return paths, so there
//! are no manual `node_free` cleanup calls (the C port had ~30 such
//! calls scattered across error exits).
//!
//! Memory ownership for named-backref / named-capture-group bytes:
//! the C port aliased pattern-buffer pointers into
//! `Node.backref_name` and `Parser.names_ptr[]`. The Rust port keeps
//! small owned copies (`Vec<u8>`) — names are typically short
//! (<32 bytes) and the alloc cost is negligible vs. the lifetime
//! gymnastics of borrowing the pattern slice through three layers
//! of `Box` ownership.
//!
//! ## Module split (each ≤ 500 LOC HARD RULE)
//!
//! - [`mod@self`] — `Parser` struct + cursor primitives +
//!   `parse_alt / concat / repeat / braced_repeat` + cross-file
//!   helpers (`read_word_name`, `read_hex_digit`,
//!   `apply_property_name`, free-fn `char_node` / `class_node`).
//! - [`atom`] — `parse_atom` + `parse_group`.
//! - [`escape`] — `parse_escape` + 7 specialized escape helpers
//!   (`\k<>`, `\xHH`, `\u…`, `\p{…}`).
//! - [`class`] — `parse_class` + range/item helpers for `[...]`.

mod atom;
mod class;
mod escape;

use crate::charclass::CharClass;
use crate::node::{Node, NodeKind, REGEX_MAX_CAPTURES};

// Flag bitset — mirrors `RE_FLAG_*` in runtime_regex.c L79-87. Only
// the u flag is observed during parse (gates `\u{HHHH..}` / `\p{}`
// extended forms). Other flags (i / g / m / s / y) are honored at
// match time, not parse time.
pub const RE_FLAG_I: u8 = 0x01;
pub const RE_FLAG_G: u8 = 0x02;
pub const RE_FLAG_M: u8 = 0x04;
pub const RE_FLAG_S: u8 = 0x08;
pub const RE_FLAG_U: u8 = 0x10;
pub const RE_FLAG_Y: u8 = 0x20;

#[derive(Debug)]
pub struct Parser<'p> {
    /// Pattern bytes (borrowed from caller — typically a `Str` payload).
    p: &'p [u8],
    /// Cursor position in `p`.
    pub(super) i: usize,
    /// Active flag set (only `RE_FLAG_U` observed by parser).
    pub(super) flags: u8,
    /// Sticky error flag — once set, the recursive descent unwinds
    /// returning `None` from each level.
    pub(super) err: bool,
    /// Capturing-group counter, bumped on every `(...)` open (NOT
    /// `(?:...)`). Index 0 is the whole-match span (reserved); user
    /// groups are 1..=n_captures.
    pub n_captures: usize,
    /// Name table for `(?<name>...)` capture groups. Indexed by
    /// `capture_idx` (1..=n_captures); slot 0 unused. Empty `Vec<u8>`
    /// = unnamed slot.
    pub names: Vec<Vec<u8>>,
}

impl<'p> Parser<'p> {
    pub fn new(pattern: &'p [u8], flags: u8) -> Self {
        // names[0] is reserved (whole-match record). Capacity sized
        // for the worst case (REGEX_MAX_CAPTURES + 1) so push never
        // reallocates regardless of how many named groups land.
        let mut names = Vec::with_capacity(REGEX_MAX_CAPTURES + 1);
        names.push(Vec::new());
        Self {
            p: pattern,
            i: 0,
            flags,
            err: false,
            n_captures: 0,
            names,
        }
    }

    /// Parse `pattern` to an AST root. Returns `None` (and sets
    /// `self.err`) on malformed input; the matcher's fallback path
    /// then treats the regex as "always-false" (matching bun's
    /// SyntaxError-at-JS-level behavior).
    pub fn parse(&mut self) -> Option<Box<Node>> {
        let root = self.parse_alt()?;
        // Any pattern bytes remaining (e.g. unbalanced `)`) is an
        // error — parse_atom rejects bare `)` at atom slot, so this
        // is mostly a defensive check.
        if self.i != self.p.len() {
            self.err = true;
            return None;
        }
        Some(root)
    }

    pub fn err(&self) -> bool {
        self.err
    }

    // ---- Low-level cursor primitives (port of p_eof/peek/get/match) ----

    pub(super) fn eof(&self) -> bool {
        self.i >= self.p.len()
    }

    pub(super) fn peek(&self) -> u8 {
        self.p[self.i]
    }

    pub(super) fn peek_at(&self, off: usize) -> u8 {
        self.p.get(self.i + off).copied().unwrap_or(0)
    }

    pub(super) fn get(&mut self) -> u8 {
        let c = self.p[self.i];
        self.i += 1;
        c
    }

    pub(super) fn match_byte(&mut self, c: u8) -> bool {
        if !self.eof() && self.peek() == c {
            self.i += 1;
            true
        } else {
            false
        }
    }

    pub(super) fn remaining(&self) -> usize {
        self.p.len() - self.i
    }

    pub(super) fn byte_at(&self, i: usize) -> u8 {
        self.p[i]
    }

    // ---- Mutually recursive grammar (alt → concat → repeat → atom) ----

    fn parse_alt(&mut self) -> Option<Box<Node>> {
        let first = self.parse_concat()?;
        if self.eof() || self.peek() != b'|' {
            return Some(first);
        }
        let mut alt = Node::new(NodeKind::Alt);
        alt.push_kid(first);
        while !self.eof() && self.peek() == b'|' {
            self.get();
            let next = self.parse_concat()?;
            alt.push_kid(next);
        }
        Some(alt)
    }

    /// Public to siblings — `parse_group` calls this after `(`.
    pub(super) fn parse_alt_for_group(&mut self) -> Option<Box<Node>> {
        self.parse_alt()
    }

    fn parse_concat(&mut self) -> Option<Box<Node>> {
        let mut seq = Node::new(NodeKind::Concat);
        while !self.eof() && self.peek() != b'|' && self.peek() != b')' {
            let a = self.parse_atom_with_repeat()?;
            seq.push_kid(a);
        }
        Some(seq)
    }

    fn parse_atom_with_repeat(&mut self) -> Option<Box<Node>> {
        let a = self.parse_atom()?;
        self.parse_repeat(a)
    }

    fn parse_repeat(&mut self, atom: Box<Node>) -> Option<Box<Node>> {
        if self.eof() {
            return Some(atom);
        }
        let c = self.peek();
        let (min, max) = match c {
            b'*' => {
                self.get();
                (0, -1)
            }
            b'+' => {
                self.get();
                (1, -1)
            }
            b'?' => {
                self.get();
                (0, 1)
            }
            b'{' => match self.parse_braced_repeat()? {
                Some(bounds) => bounds,
                None => return Some(atom),
            },
            _ => return Some(atom),
        };
        let lazy = self.match_byte(b'?');
        let mut r = Node::new(NodeKind::Repeat);
        r.child = Some(atom);
        r.min = min;
        r.max = max;
        r.lazy = lazy;
        Some(r)
    }

    /// Parse `{n}` / `{n,}` / `{n,m}` after the `{` has been peeked
    /// (not yet consumed). Returns `Some(Some(bounds))` on a valid
    /// quantifier (advancing past `}`); `Some(None)` when the brace
    /// turned out not to form a valid quantifier and the cursor was
    /// rolled back (caller treats `{` as literal — matches JS Annex
    /// B). `None` on hard error.
    fn parse_braced_repeat(&mut self) -> Option<Option<(i32, i32)>> {
        let save = self.i;
        self.get(); // consume `{`
        if self.eof() || !self.peek().is_ascii_digit() {
            self.i = save;
            return Some(None);
        }
        let mut n1 = 0i32;
        while !self.eof() && self.peek().is_ascii_digit() {
            n1 = n1 * 10 + (self.get() - b'0') as i32;
        }
        if self.eof() {
            self.i = save;
            return Some(None);
        }
        if self.peek() == b'}' {
            self.get();
            return Some(Some((n1, n1)));
        }
        if self.peek() != b',' {
            self.i = save;
            return Some(None);
        }
        self.get(); // consume `,`
        if !self.eof() && self.peek() == b'}' {
            self.get();
            return Some(Some((n1, -1)));
        }
        if self.eof() || !self.peek().is_ascii_digit() {
            self.i = save;
            return Some(None);
        }
        let mut n2 = 0i32;
        while !self.eof() && self.peek().is_ascii_digit() {
            n2 = n2 * 10 + (self.get() - b'0') as i32;
        }
        if self.eof() || self.peek() != b'}' {
            self.i = save;
            return Some(None);
        }
        self.get(); // consume `}`
        Some(Some((n1, n2)))
    }

    // ---- Shared helpers used by atom / escape / class siblings ----

    /// Read a sequence of word bytes (`[A-Za-z0-9_]`) terminated by
    /// `delim`. Consumes the delimiter. Returns `None` (sets err) on
    /// EOF or empty name.
    pub(super) fn read_word_name(&mut self, delim: u8) -> Option<Vec<u8>> {
        let start = self.i;
        while !self.eof() && self.peek() != delim {
            if !is_word_byte(self.peek()) {
                self.err = true;
                return None;
            }
            self.get();
        }
        if self.eof() {
            self.err = true;
            return None;
        }
        let name = self.p[start..self.i].to_vec();
        if name.is_empty() {
            self.err = true;
            return None;
        }
        self.get(); // consume delim
        Some(name)
    }

    /// Consume one hex digit. Returns nibble value 0..=15 or `None`
    /// (sets err) on EOF or non-hex byte.
    pub(super) fn read_hex_digit(&mut self) -> Option<u8> {
        if self.eof() {
            self.err = true;
            return None;
        }
        let h = self.get();
        match hex_value(h) {
            Some(v) => Some(v),
            None => {
                self.err = true;
                None
            }
        }
    }

    pub(super) fn set_err(&mut self) {
        self.err = true;
    }
}

// ---- Free helpers shared across parser/{atom,escape,class} ----

pub(super) fn char_node(ch: u8) -> Box<Node> {
    let mut n = Node::new(NodeKind::Char);
    n.ch = ch;
    n
}

pub(super) fn class_node<F: FnOnce(&mut CharClass)>(populate: F) -> Box<Node> {
    let mut n = Node::new(NodeKind::Class);
    populate(&mut n.cc);
    n
}

pub(super) fn hex_value(h: u8) -> Option<u8> {
    if h.is_ascii_digit() {
        Some(h - b'0')
    } else if (b'a'..=b'f').contains(&h) {
        Some(h - b'a' + 10)
    } else if (b'A'..=b'F').contains(&h) {
        Some(h - b'A' + 10)
    } else {
        None
    }
}

/// Word-byte predicate used by named-group / `\k<>` / `\p{}` name
/// parsing. Matches `[A-Za-z0-9_]` (ASCII only — same as C port).
/// Public so the future VM (P6.2-e) can reuse for `\b` word-boundary.
pub fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Apply a Unicode property name (`L` / `Letter` / `N` / `Number` /
/// `ASCII`) to the char-class on `n`. Returns `false` on unknown name.
pub(super) fn apply_property_name(n: &mut Node, name: &[u8]) -> bool {
    match name {
        b"L" | b"Letter" => {
            n.cc.add_property_letter();
            true
        }
        b"N" | b"Number" => {
            n.cc.add_property_number();
            true
        }
        b"ASCII" => {
            n.cc.add_property_ascii();
            true
        }
        _ => false,
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
    fn parses_single_literal() {
        let r = parse_ok("a", 0);
        assert_eq!(r.kind, NodeKind::Concat);
        assert_eq!(r.kids.len(), 1);
        assert_eq!(r.kids[0].kind, NodeKind::Char);
        assert_eq!(r.kids[0].ch, b'a');
    }

    #[test]
    fn parses_concat() {
        let r = parse_ok("abc", 0);
        assert_eq!(r.kids.len(), 3);
        assert_eq!(
            r.kids.iter().map(|k| k.ch).collect::<Vec<_>>(),
            vec![b'a', b'b', b'c']
        );
    }

    #[test]
    fn parses_alternation() {
        let r = parse_ok("a|b|c", 0);
        assert_eq!(r.kind, NodeKind::Alt);
        assert_eq!(r.kids.len(), 3);
    }

    #[test]
    fn parses_star_plus_question() {
        for (pat, min, max) in [("a*", 0, -1), ("a+", 1, -1), ("a?", 0, 1)] {
            let r = parse_ok(pat, 0);
            let rep = &r.kids[0];
            assert_eq!(rep.kind, NodeKind::Repeat);
            assert_eq!(rep.min, min);
            assert_eq!(rep.max, max);
            assert!(!rep.lazy);
        }
    }

    #[test]
    fn parses_lazy_quantifiers() {
        let r = parse_ok("a*?", 0);
        assert!(r.kids[0].lazy);
    }

    #[test]
    fn parses_braced_repeat_forms() {
        for (pat, min, max) in [("a{3}", 3, 3), ("a{2,}", 2, -1), ("a{2,5}", 2, 5)] {
            let r = parse_ok(pat, 0);
            let rep = &r.kids[0];
            assert_eq!(rep.kind, NodeKind::Repeat);
            assert_eq!(rep.min, min);
            assert_eq!(rep.max, max);
        }
    }

    #[test]
    fn parses_braced_invalid_as_literal() {
        // `{o}` is not a valid quantifier → `{` is literal, then `o`,
        // then `}`. Pattern length = 3 literal chars + leading `a`.
        let r = parse_ok("a{o}", 0);
        assert_eq!(r.kids.len(), 4);
        assert_eq!(r.kids[1].ch, b'{');
        assert_eq!(r.kids[2].ch, b'o');
        assert_eq!(r.kids[3].ch, b'}');
    }

    #[test]
    fn parses_dot_any() {
        let r = parse_ok(".", 0);
        assert_eq!(r.kids[0].kind, NodeKind::Any);
    }

    #[test]
    fn parses_anchors() {
        let r = parse_ok("^a$", 0);
        assert_eq!(r.kids[0].kind, NodeKind::AnchorBeg);
        assert_eq!(r.kids[2].kind, NodeKind::AnchorEnd);
    }

    #[test]
    fn rejects_dangling_quantifier() {
        parse_err("*", 0);
        parse_err("+", 0);
        parse_err("?", 0);
    }

    #[test]
    fn parses_alternation_with_quantifier() {
        let r = parse_ok("a+|b+", 0);
        assert_eq!(r.kind, NodeKind::Alt);
        assert_eq!(r.kids.len(), 2);
    }

    #[test]
    fn captures_dot_after_concat() {
        let r = parse_ok("a.b", 0);
        assert_eq!(r.kids[1].kind, NodeKind::Any);
    }

    #[test]
    fn is_word_byte_covers_ascii_word_chars() {
        for c in (b'a'..=b'z').chain(b'A'..=b'Z').chain(b'0'..=b'9') {
            assert!(is_word_byte(c));
        }
        assert!(is_word_byte(b'_'));
        for c in [b' ', b'-', 0x80] {
            assert!(!is_word_byte(c));
        }
    }
}
