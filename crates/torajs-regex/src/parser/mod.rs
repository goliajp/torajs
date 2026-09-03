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
//!   `read_group_name` in [`group_name`],
//!   `apply_property_name`, free-fn `char_node` / `class_node`).
//! - [`atom`] — `parse_atom` + `parse_group`.
//! - [`escape`] — `parse_escape` + 7 specialized escape helpers
//!   (`\k<>`, `\xHH`, `\u…`, `\p{…}`).
//! - [`class`] — `parse_class` + range/item helpers for `[...]`.
//! - [`class_v`] — v-flag `ClassSetExpression` (`[...]` under
//!   unicodeSets: nested classes + `&&` / `--` set algebra +
//!   `\q{}` strings); [`class_v_set`] — its set value + fold
//!   helpers.

mod atom;
mod class;
mod class_v;
mod class_v_set;
mod escape;
mod group_name;
mod helpers;
mod named_groups;

pub use helpers::is_word_byte;
pub(crate) use helpers::{apply_property_name, char_node, class_node, hex_value};

use crate::node::{Node, NodeKind};
use alloc::{boxed::Box, vec::Vec};

// Flag bitset — mirrors `RE_FLAG_*` in runtime_regex.c L79-87. The
// u flag gates parse-time forms (`\u{HHHH..}` / `\p{}`). i / m / s
// are resolved at parse time into per-atom `Node::eff_ims` (merged
// with `(?ims-ims:…)` modifier groups) and baked into `Inst.pad` by
// the compiler; g / y stay match-time surface flags.
pub const RE_FLAG_I: u8 = 0x01;
pub const RE_FLAG_G: u8 = 0x02;
pub const RE_FLAG_M: u8 = 0x04;
pub const RE_FLAG_S: u8 = 0x08;
pub const RE_FLAG_U: u8 = 0x10;
pub const RE_FLAG_Y: u8 = 0x20;
pub const RE_FLAG_V: u8 = 0x40;
/// `d` (hasIndices, §22.2.7.8 MakeIndicesArray) — pure match-time
/// surface flag: gates the `.indices` property on exec-shape match
/// results, never observed by parser or compiler.
pub const RE_FLAG_D: u8 = 0x80;

/// True iff the pattern is in Unicode mode — the `u` OR `v`
/// (unicodeSets) flag. Every parse / match decision that used to gate
/// on `RE_FLAG_U` alone (cp decode, `\u{}` / `\p{}` forms, lone
/// surrogate rejection, strict backrefs) applies identically under
/// `v`, which is a superset of `u` semantics.
#[inline]
pub fn unicode_mode(flags: u8) -> bool {
    flags & (RE_FLAG_U | RE_FLAG_V) != 0
}

/// The three flag bits an inline modifier group `(?ims-ims:…)` may
/// toggle (ES 2025 regexp-modifiers).
pub const RE_FLAGS_IMS: u8 = RE_FLAG_I | RE_FLAG_M | RE_FLAG_S;

#[derive(Debug)]
pub struct Parser<'p> {
    /// Pattern bytes (borrowed from caller — typically a `Str` payload).
    p: &'p [u8],
    /// Cursor position in `p`.
    pub(super) i: usize,
    /// Active flag set (only `RE_FLAG_U` observed by parser).
    pub(super) flags: u8,
    /// Effective `i`/`m`/`s` bits at the cursor — the global flags
    /// merged with every enclosing `(?ims-ims:…)` modifier group.
    /// Saved/restored around each modifier group's body by
    /// `parse_group`; stamped onto every atom in
    /// `parse_atom_with_repeat`.
    pub(super) eff_ims: u8,
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
    /// Pre-scan flag — set at [`Parser::new`] via
    /// [`scan_has_named_groups`]. Gates the annexB §B.1.4
    /// `\k<name>` identity fallback: outside u/v mode, a `\k` in a
    /// pattern with NO `(?<name>...)` group anywhere acts as a
    /// literal `k` and the trailing `<name>` reparses as literals
    /// (per test262 `RegExp/named-groups/non-unicode-malformed.js`).
    /// A pattern with even one named group is strict: `\k<x>` where
    /// `x` isn't defined is a SyntaxError. u/v mode is strict
    /// regardless.
    pub(super) has_named_groups: bool,
}

impl<'p> Parser<'p> {
    pub fn new(pattern: &'p [u8], flags: u8) -> Self {
        // names[0] is reserved (whole-match record). Chunk 801 —
        // the capture cap became a 65536 sanity bound, so capacity
        // no longer pre-sizes to it; a small seed covers typical
        // patterns and the Vec grows for the rest.
        let mut names = Vec::with_capacity(8);
        names.push(Vec::new());
        let has_named_groups = named_groups::scan_has_named_groups(pattern);
        Self {
            p: pattern,
            i: 0,
            flags,
            eff_ims: flags & RE_FLAGS_IMS,
            err: false,
            n_captures: 0,
            names,
            has_named_groups,
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
        // ES §22.2.1.1 Static Semantics: dup GroupName in the same
        // Alternative is a SyntaxError (ES2025 disjunction-siblings
        // carve-out applies). See `named_groups`.
        if !named_groups::check_named_groups_disjunction_ok(&root, &self.names) {
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
        let mut a = self.parse_atom()?;
        // Stamp the effective i/m/s bits onto the atom. Nested atoms
        // (group / lookaround bodies) were stamped by their own
        // recursive descent — under the body's own modifier scope —
        // so this only records the container node's outer scope
        // (unused for non-leaf kinds; the compiler reads leaves).
        a.eff_ims = self.eff_ims;
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
        // §22.2.1.1 Quantifier / annexB QuantifiableAssertion:
        // - Lookbehind (`(?<=…)`, `(?<!…)`) with a quantifier is a
        //   SyntaxError in every mode — the annexB
        //   QuantifiableAssertion carve-out covers *lookahead only*
        //   (`(?= …)`, `(?! …)`).
        // - Lookahead + quantifier is annexB-legal in non-u (`/(?=a)+/`
        //   accepts) but strict u/v rejects.
        match atom.kind {
            NodeKind::Lookbehind | NodeKind::NegLookbehind => {
                self.err = true;
                return None;
            }
            NodeKind::Lookahead | NodeKind::NegLookahead if unicode_mode(self.flags) => {
                self.err = true;
                return None;
            }
            _ => {}
        }
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
        // ES §22.2.1.1 Term / QuantifierPrefix Static Semantics —
        // `{n,m}` requires `n <= m`. bun/JSC throw SyntaxError from
        // the constructor for `a{2,1}` unconditionally (u and non-u).
        // Pre-fix tr accepted the reversed pair and produced a
        // no-match matcher; make it a hard parse err so the throw
        // plumbing (fee80fe5) surfaces the SyntaxError to
        // `try/catch`. Both bounds are consumed at this point (past
        // `}`), so no rewind — the rejection is final, not annexB.
        if n2 < n1 {
            self.err = true;
            return None;
        }
        Some(Some((n1, n2)))
    }

    // ---- Shared helpers used by atom / escape / class siblings ----

    /// Read a sequence of word bytes (`[A-Za-z0-9_]`) terminated by
    /// `delim`. Consumes the delimiter. Returns `None` (sets err) on
    /// EOF or empty name. This is the `\p{Name=Value}` value
    /// production — a capture-group name is an identifier and reads
    /// through [`Parser::read_group_name`] instead.
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

    /// Read the `Name` or `Name=Value` body of a `\p{...}` escape,
    /// consuming the closing `}`. Both parts are word-byte sequences;
    /// returns `(name, None)` for the lone form and
    /// `(name, Some(value))` for the keyed form. `None` (sets err) on
    /// EOF, empty name, or a non-word byte.
    pub(super) fn read_property_expr(&mut self) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
        let start = self.i;
        while !self.eof() && self.peek() != b'}' && self.peek() != b'=' {
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
        if self.get() == b'}' {
            return Some((name, None));
        }
        // consumed `=` — the value part runs to `}`.
        let value = self.read_word_name(b'}')?;
        Some((name, Some(value)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

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
