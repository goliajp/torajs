//! Atom + group parsing — extracted from `runtime_regex.c` L825-933.
//!
//! `parse_atom` dispatches the leading byte of an atom:
//! `(` / `[` / `.` / `^` / `$` / `\` / quantifier-without-leading-atom
//! / plain literal. Group sub-syntaxes (`(?:`, `(?=`, `(?!`, `(?<=`,
//! `(?<!`, `(?<name>`, `(`) flow through `parse_group`.

use super::Parser;
use crate::node::{Node, NodeKind, REGEX_MAX_CAPTURES};
use alloc::{boxed::Box, vec::Vec};

impl<'p> Parser<'p> {
    pub(super) fn parse_atom(&mut self) -> Option<Box<Node>> {
        if self.eof() {
            self.set_err();
            return None;
        }
        let c = self.peek();
        match c {
            b'(' => {
                self.get();
                self.parse_group()
            }
            b'[' => {
                self.get();
                if self.flags & crate::parser::RE_FLAG_V != 0 {
                    self.parse_class_v()
                } else {
                    self.parse_class()
                }
            }
            b'.' => {
                self.get();
                Some(Node::new(NodeKind::Any))
            }
            b'^' => {
                self.get();
                Some(Node::new(NodeKind::AnchorBeg))
            }
            b'$' => {
                self.get();
                Some(Node::new(NodeKind::AnchorEnd))
            }
            b'\\' => {
                self.get();
                self.parse_escape()
            }
            b')' | b'|' | b'*' | b'+' | b'?' => {
                self.set_err();
                None
            }
            b'{' | b'}' | b']' if super::unicode_mode(self.flags) => {
                // §22.2.1 Pattern[+UnicodeMode] — `{`, `}`, `]` are
                // SyntaxCharacters, and a SyntaxCharacter is not a
                // PatternCharacter; the annexB
                // ExtendedPatternCharacter escape hatch that lets
                // them read as literals exists only OUTSIDE Unicode
                // mode. Bare occurrences here (a malformed brace
                // body like `/x{/u`, a stray `}` or `]`) are
                // SyntaxErrors under u/v.
                self.set_err();
                None
            }
            b'{' if lookahead_valid_brace_quantifier(self.p, self.i) => {
                // §22.2.1.1 Term / Quantifier Early Error: a
                // well-formed `{n}` / `{n,}` / `{n,m}` here has no
                // preceding Atom to bind to → "Nothing to repeat"
                // SyntaxError, every mode (bun/JSC reject both non-u
                // and u). A malformed brace body (`{}`, `{a}`,
                // `{,3}`, `{` at EOF) still falls through to the
                // annexB literal `{` path below.
                self.set_err();
                None
            }
            _ => {
                self.get();
                Some(super::char_node(c))
            }
        }
    }

    /// Parse the body after `(` has been consumed. Dispatches on the
    /// `?...` prefix variants (non-capturing, modifier group,
    /// lookahead/behind, named capture) and falls through to plain
    /// capturing group.
    fn parse_group(&mut self) -> Option<Box<Node>> {
        let mut kind = NodeKind::Group;
        let mut capture_idx: i32 = -1;
        let mut saved_eff: Option<u8> = None;
        if !self.eof() && self.peek() == b'?' {
            let after = self.peek_at(1);
            match after {
                b':' => {
                    self.get();
                    self.get();
                }
                b'i' | b'm' | b's' | b'-' => {
                    // `(?ims-ims:…)` — ES 2025 regexp-modifiers. Both
                    // flag runs restrict to i/m/s; only the group form
                    // (`:` terminator) exists in the spec. The body
                    // parses under the updated effective set, restored
                    // after `)`.
                    self.get(); // consume `?`
                    let (add, remove) = self.parse_modifier_flags()?;
                    saved_eff = Some(self.eff_ims);
                    self.eff_ims = (self.eff_ims | add) & !remove;
                }
                b'=' => {
                    self.get();
                    self.get();
                    kind = NodeKind::Lookahead;
                }
                b'!' => {
                    self.get();
                    self.get();
                    kind = NodeKind::NegLookahead;
                }
                b'<' => match self.peek_at(2) {
                    b'=' => {
                        self.get();
                        self.get();
                        self.get();
                        kind = NodeKind::Lookbehind;
                    }
                    b'!' => {
                        self.get();
                        self.get();
                        self.get();
                        kind = NodeKind::NegLookbehind;
                    }
                    name_lead if super::is_word_byte(name_lead) => {
                        // `(?<name>...)` — named capture group.
                        self.get();
                        self.get(); // consume `?<`
                        let name = self.read_word_name(b'>')?;
                        capture_idx = self.assign_capture_idx()?;
                        // Ensure the names slot exists at this index.
                        while self.names.len() <= capture_idx as usize {
                            self.names.push(Vec::new());
                        }
                        self.names[capture_idx as usize] = name;
                    }
                    _ => {
                        self.set_err();
                        return None;
                    }
                },
                _ => {
                    self.set_err();
                    return None;
                }
            }
        } else {
            capture_idx = self.assign_capture_idx()?;
        }
        let inner = self.parse_alt_for_group()?;
        if let Some(outer) = saved_eff {
            self.eff_ims = outer;
        }
        if !self.match_byte(b')') {
            self.set_err();
            return None;
        }
        let mut g = Node::new(kind);
        g.child = Some(inner);
        g.capture_idx = capture_idx;
        Some(g)
    }

    /// Parse the `ims-ims:` tail of a modifier group — cursor sits on
    /// the first flag letter (or `-`). Consumes through the `:`.
    /// Returns `(add, remove)` bit masks. Errors (ES early errors):
    /// a letter outside `i`/`m`/`s`, a duplicate within either run, a
    /// letter present in both runs, or both runs empty.
    fn parse_modifier_flags(&mut self) -> Option<(u8, u8)> {
        let add = self.read_modifier_run()?;
        let remove = if self.match_byte(b'-') {
            self.read_modifier_run()?
        } else {
            0
        };
        if !self.match_byte(b':') || add & remove != 0 || (add == 0 && remove == 0) {
            self.set_err();
            return None;
        }
        Some((add, remove))
    }

    /// One run of modifier letters (`i` / `m` / `s`, no duplicates).
    /// Stops at the first non-letter byte without consuming it.
    fn read_modifier_run(&mut self) -> Option<u8> {
        let mut mask = 0u8;
        loop {
            let bit = match if self.eof() { 0 } else { self.peek() } {
                b'i' => crate::parser::RE_FLAG_I,
                b'm' => crate::parser::RE_FLAG_M,
                b's' => crate::parser::RE_FLAG_S,
                b'-' | b':' => return Some(mask),
                _ => {
                    self.set_err();
                    return None;
                }
            };
            if mask & bit != 0 {
                self.set_err();
                return None;
            }
            mask |= bit;
            self.get();
        }
    }

    /// Increment `n_captures` and return the new 1-based index, or
    /// `None` (sets err) if it would exceed the save-slot budget.
    ///
    /// The cap is `REGEX_MAX_CAPTURES - 1` user groups — a 65535
    /// sanity bound (V8's kMaxCaptures), not a buffer limit: save
    /// rows are stride-sized per program since chunk 801, so any
    /// accepted group count fits its own row. Rejected pathological
    /// patterns take the `rejected` path — miss for test/find,
    /// `abort_unsupported` subset boundary for heavier surfaces.
    fn assign_capture_idx(&mut self) -> Option<i32> {
        self.n_captures += 1;
        let idx = self.n_captures as i32;
        if idx >= REGEX_MAX_CAPTURES as i32 {
            self.set_err();
            return None;
        }
        Some(idx)
    }
}

/// Peek at `p[i..]` (where `p[i]` is `{`) and return true iff it
/// forms a well-formed `{n}` / `{n,}` / `{n,m}` quantifier. Used
/// by `parse_atom` to distinguish "Quantifier with no preceding
/// Atom" SyntaxError (well-formed brace) from annexB literal-`{`
/// fallback (malformed body).
fn lookahead_valid_brace_quantifier(p: &[u8], mut i: usize) -> bool {
    if i >= p.len() || p[i] != b'{' {
        return false;
    }
    i += 1;
    let digits_start = i;
    while i < p.len() && p[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits_start {
        return false;
    }
    if i < p.len() && p[i] == b'}' {
        return true;
    }
    if i >= p.len() || p[i] != b',' {
        return false;
    }
    i += 1;
    while i < p.len() && p[i].is_ascii_digit() {
        i += 1;
    }
    i < p.len() && p[i] == b'}'
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
    fn parses_capturing_group_assigns_indices() {
        let r = parse_ok("(a)(b)", 0);
        assert_eq!(r.kids[0].kind, NodeKind::Group);
        assert_eq!(r.kids[0].capture_idx, 1);
        assert_eq!(r.kids[1].capture_idx, 2);
    }

    #[test]
    fn parses_non_capturing_group() {
        let r = parse_ok("(?:a)", 0);
        assert_eq!(r.kids[0].kind, NodeKind::Group);
        assert_eq!(r.kids[0].capture_idx, -1);
    }

    #[test]
    fn parses_named_capture() {
        let mut p = Parser::new(b"(?<year>\\d+)", 0);
        let r = p.parse().expect("parse");
        assert_eq!(r.kids[0].capture_idx, 1);
        assert_eq!(&p.names[1], b"year");
    }

    #[test]
    fn parses_lookahead_and_lookbehind() {
        for (pat, kind) in [
            ("(?=a)", NodeKind::Lookahead),
            ("(?!a)", NodeKind::NegLookahead),
            ("(?<=a)", NodeKind::Lookbehind),
            ("(?<!a)", NodeKind::NegLookbehind),
        ] {
            let r = parse_ok(pat, 0);
            assert_eq!(r.kids[0].kind, kind);
        }
    }

    #[test]
    fn parses_nested_groups_assign_in_source_order() {
        let r = parse_ok("((a)b)", 0);
        let outer = &r.kids[0];
        assert_eq!(outer.kind, NodeKind::Group);
        assert_eq!(outer.capture_idx, 1);
        let inner_concat = outer.child.as_ref().expect("inner");
        let inner_group = &inner_concat.kids[0];
        assert_eq!(inner_group.kind, NodeKind::Group);
        assert_eq!(inner_group.capture_idx, 2);
    }

    #[test]
    fn rejects_unbalanced_paren() {
        parse_err("(a", 0);
    }

    #[test]
    fn rejects_unknown_paren_prefix() {
        parse_err("(?@)", 0);
    }

    #[test]
    fn accepts_many_capture_groups() {
        // Chunk 801 retired the fixed 32-group cap — save rows are
        // stride-sized, so counts past the old buffer boundary parse
        // and carry their indices.
        let pat: alloc::string::String = (0..100).map(|_| "(a)").collect();
        let mut p = Parser::new(pat.as_bytes(), 0);
        assert!(p.parse().is_some() && !p.err());
        assert_eq!(p.n_captures, 100);
    }

    #[test]
    fn modifier_group_updates_eff_ims_and_restores() {
        // `(?i:a)b` — 'a' carries the i bit, 'b' does not.
        use crate::parser::{RE_FLAG_I, RE_FLAG_M, RE_FLAG_S};
        let r = parse_ok("(?i:a)b", 0);
        let group = &r.kids[0];
        assert_eq!(group.kind, NodeKind::Group);
        assert_eq!(group.capture_idx, -1);
        let inner = group.child.as_ref().expect("inner");
        assert_eq!(inner.kids[0].eff_ims, RE_FLAG_I);
        assert_eq!(r.kids[1].eff_ims, 0);
        // Remove form: global i, `(?-i:a)` clears it inside only.
        let r = parse_ok("(?-i:a)b", RE_FLAG_I);
        let inner = r.kids[0].child.as_ref().expect("inner");
        assert_eq!(inner.kids[0].eff_ims, 0);
        assert_eq!(r.kids[1].eff_ims, RE_FLAG_I);
        // Combined add/remove + nesting.
        let r = parse_ok("(?ms-i:(?i:a)b)", RE_FLAG_I);
        let outer = r.kids[0].child.as_ref().expect("outer");
        let nested = outer.kids[0].child.as_ref().expect("nested");
        assert_eq!(nested.kids[0].eff_ims, RE_FLAG_I | RE_FLAG_M | RE_FLAG_S);
        assert_eq!(outer.kids[1].eff_ims, RE_FLAG_M | RE_FLAG_S);
        // Empty remove run after `-` is allowed.
        parse_ok("(?i-:a)", 0);
    }

    #[test]
    fn modifier_group_syntax_errors() {
        // Bare `(?i)` (no `:`), non-ims letters, duplicates, overlap,
        // both-empty.
        for pat in [
            "(?i)",
            "(?g:a)",
            "(?u:a)",
            "(?ii:a)",
            "(?i-i:a)",
            "(?-:a)",
            "(?im-mi:a)",
        ] {
            parse_err(pat, 0);
        }
    }

    #[test]
    fn rejects_past_sanity_cap() {
        // REGEX_MAX_CAPTURES (65536, V8's kMaxCaptures) stays as a
        // pathological-pattern guard: group 65536 is rejected.
        let pat: alloc::string::String = (0..65536).map(|_| "(a)").collect();
        parse_err(&pat, 0);
    }
}
