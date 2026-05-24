//! Regex AST — port of `runtime_regex.c` L351-429.
//!
//! Recursive tree produced by the parser, compiled to flat bytecode
//! by the Thompson-construction pass. Memory ownership is
//! `Vec<Box<Node>> + Option<Box<Node>>` — Rust's `Drop` recursively
//! frees the entire tree (the C port had a manual `node_free`).

use crate::charclass::CharClass;

/// Maximum number of capture groups in one regex. Indices 1..=N are
/// user groups; index 0 is reserved for the whole-match span. The
/// parser rejects patterns with more than this many `(...)` and the
/// matcher allocates `2 * (N+1)` save slots per thread.
pub const REGEX_MAX_CAPTURES: usize = 32;

/// `2 * REGEX_MAX_CAPTURES` — width of the Thread.saves array.
pub const REGEX_SAVE_SLOTS: usize = REGEX_MAX_CAPTURES * 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Char,
    Any,
    Class,
    AnchorBeg,
    AnchorEnd,
    WBound,
    NWBound,
    Concat,
    Alt,
    Repeat,
    /// Group — either capturing (`capture_idx >= 1`) or non-capturing
    /// `(?:...)` (`capture_idx == -1`).
    Group,
    /// `(?=X)` — zero-width positive assertion.
    Lookahead,
    /// `(?!X)` — zero-width negative assertion.
    NegLookahead,
    /// `(?<=X)` — zero-width positive lookbehind.
    Lookbehind,
    /// `(?<!X)` — zero-width negative lookbehind.
    NegLookbehind,
    /// `\N` decimal or `\k<name>` — references capture N.
    Backref,
}

#[derive(Debug)]
pub struct Node {
    pub kind: NodeKind,

    /// `Char` — the literal byte.
    pub ch: u8,

    /// `Class` — owned character class.
    pub cc: CharClass,

    /// `Concat` / `Alt` — ordered children.
    pub kids: Vec<Box<Node>>,

    /// `Repeat` — min count and max count (`-1` = unbounded).
    pub min: i32,
    pub max: i32,

    /// `Repeat` — `false` = greedy, `true` = lazy (`*?`, `+?`, etc.).
    pub lazy: bool,

    /// `Repeat`, `Group`, `Lookahead`, `NegLookahead`, `Lookbehind`,
    /// `NegLookbehind` — single child sub-pattern.
    pub child: Option<Box<Node>>,

    /// `Group` — capture index (1-based; `0` reserved for whole-match).
    /// `-1` = non-capturing `(?:...)`.
    ///
    /// `Backref` — references this capture index (1..=n_captures).
    /// `-1` means unresolved named ref (look up via `backref_name`
    /// post-parse).
    pub capture_idx: i32,

    /// `Backref` named — captured name copy. Empty for unnamed `\N`
    /// backrefs and non-Backref nodes. (C port aliased pattern bytes;
    /// the Rust port keeps a small owned copy so the Node tree has no
    /// lifetime tied to the pattern buffer — names are short, so the
    /// alloc cost is negligible.)
    pub backref_name: Vec<u8>,
}

impl Node {
    pub fn new(kind: NodeKind) -> Box<Node> {
        Box::new(Node {
            kind,
            ch: 0,
            cc: CharClass::new(),
            kids: Vec::new(),
            min: 0,
            max: -1,
            lazy: false,
            child: None,
            capture_idx: -1,
            backref_name: Vec::new(),
        })
    }

    pub fn push_kid(&mut self, kid: Box<Node>) {
        self.kids.push(kid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults_match_c_port() {
        let n = Node::new(NodeKind::Char);
        assert_eq!(n.kind, NodeKind::Char);
        assert_eq!(n.ch, 0);
        assert_eq!(n.min, 0);
        assert_eq!(n.max, -1);
        assert!(!n.lazy);
        assert!(n.child.is_none());
        assert_eq!(n.capture_idx, -1);
        assert!(n.backref_name.is_empty());
        assert!(n.kids.is_empty());
    }

    #[test]
    fn push_kid_grows_vec() {
        let mut parent = Node::new(NodeKind::Concat);
        for i in 0..10 {
            let mut kid = Node::new(NodeKind::Char);
            kid.ch = b'a' + i;
            parent.push_kid(kid);
        }
        assert_eq!(parent.kids.len(), 10);
        for (i, kid) in parent.kids.iter().enumerate() {
            assert_eq!(kid.ch, b'a' + i as u8);
        }
    }

    #[test]
    fn drop_recursively_frees_nested_tree() {
        let mut root = Node::new(NodeKind::Alt);
        let mut left = Node::new(NodeKind::Concat);
        for c in [b'a', b'b', b'c'] {
            let mut leaf = Node::new(NodeKind::Char);
            leaf.ch = c;
            left.push_kid(leaf);
        }
        let mut right = Node::new(NodeKind::Repeat);
        right.min = 1;
        right.max = 3;
        let mut inner = Node::new(NodeKind::Class);
        inner.cc.add_digit();
        right.child = Some(inner);
        root.push_kid(left);
        root.push_kid(right);
        // No assertion — the test passes if `drop(root)` doesn't UAF
        // or leak under miri / address sanitizer (and the same code
        // path mirrors C's `node_free` recursion).
        drop(root);
    }

    #[test]
    fn capture_idx_starts_unresolved() {
        let n = Node::new(NodeKind::Group);
        assert_eq!(n.capture_idx, -1);
    }

    #[test]
    fn backref_name_can_be_set() {
        let mut n = Node::new(NodeKind::Backref);
        n.backref_name.extend_from_slice(b"year");
        assert_eq!(&n.backref_name, b"year");
    }

    #[test]
    fn constants_match_c_port() {
        assert_eq!(REGEX_MAX_CAPTURES, 32);
        assert_eq!(REGEX_SAVE_SLOTS, 64);
    }
}
