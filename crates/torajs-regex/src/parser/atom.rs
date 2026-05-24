//! Atom + group parsing — extracted from `runtime_regex.c` L825-933.
//!
//! `parse_atom` dispatches the leading byte of an atom:
//! `(` / `[` / `.` / `^` / `$` / `\` / quantifier-without-leading-atom
//! / plain literal. Group sub-syntaxes (`(?:`, `(?=`, `(?!`, `(?<=`,
//! `(?<!`, `(?<name>`, `(`) flow through `parse_group`.

use super::Parser;
use crate::node::{Node, NodeKind, REGEX_MAX_CAPTURES};

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
                self.parse_class()
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
            _ => {
                // Bare `{` is left to parse_repeat's lookahead — its
                // rollback handles `x{o}` / `x{` style invalid
                // quantifiers by treating `{` as literal. So here we
                // just emit the literal char.
                self.get();
                Some(super::char_node(c))
            }
        }
    }

    /// Parse the body after `(` has been consumed. Dispatches on the
    /// `?...` prefix variants (non-capturing, lookahead/behind, named
    /// capture) and falls through to plain capturing group.
    fn parse_group(&mut self) -> Option<Box<Node>> {
        let mut kind = NodeKind::Group;
        let mut capture_idx: i32 = -1;
        if !self.eof() && self.peek() == b'?' {
            let after = self.peek_at(1);
            match after {
                b':' => {
                    self.get();
                    self.get();
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
        if !self.match_byte(b')') {
            self.set_err();
            return None;
        }
        let mut g = Node::new(kind);
        g.child = Some(inner);
        g.capture_idx = capture_idx;
        Some(g)
    }

    /// Increment `n_captures` and return the new 1-based index, or
    /// `None` (sets err) if it would exceed `REGEX_MAX_CAPTURES`.
    fn assign_capture_idx(&mut self) -> Option<i32> {
        self.n_captures += 1;
        let idx = self.n_captures as i32;
        if idx > REGEX_MAX_CAPTURES as i32 {
            self.set_err();
            return None;
        }
        Some(idx)
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
}
