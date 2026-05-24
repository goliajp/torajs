//! Post-parse backref resolution — port of `runtime_regex.c`
//! L1411-1439.
//!
//! Two backref forms land in the AST as [`NodeKind::Backref`]:
//!
//! - **Decimal** `\N` — parser assigns `capture_idx = N` directly.
//!   Validated post-parse against `n_captures` (forward references
//!   are fine).
//! - **Named** `\k<name>` — parser sets `capture_idx = -1` and
//!   stores the name bytes in `Node.backref_name`. Resolution looks
//!   up the name in the parser's name table to find the matching
//!   capture index.
//!
//! Returns `true` on success (all backrefs validated/resolved),
//! `false` on the first unresolved reference encountered (named ref
//! to unknown name, or positional `\N` where `N > n_captures` — the
//! ECMA Annex B `OctalEscape` / `IdentityEscape` fallback for the
//! positional case is an L3b follow-up).

use crate::node::{Node, NodeKind};

/// Walk `node` recursively, validating + resolving every Backref.
/// `names` is indexed by capture_idx 1..=n_captures (slot 0 unused);
/// an empty `Vec<u8>` at a slot means that capture has no name.
pub fn resolve_backrefs(node: &mut Node, names: &[Vec<u8>], n_captures: usize) -> bool {
    if node.kind == NodeKind::Backref && !resolve_one(node, names, n_captures) {
        return false;
    }
    if let Some(child) = node.child.as_deref_mut()
        && !resolve_backrefs(child, names, n_captures)
    {
        return false;
    }
    for kid in &mut node.kids {
        if !resolve_backrefs(kid, names, n_captures) {
            return false;
        }
    }
    true
}

fn resolve_one(node: &mut Node, names: &[Vec<u8>], n_captures: usize) -> bool {
    if !node.backref_name.is_empty() {
        for i in 1..=n_captures {
            if names.get(i).is_some_and(|n| n == &node.backref_name) {
                node.capture_idx = i as i32;
                node.backref_name.clear();
                return true;
            }
        }
        false
    } else {
        node.capture_idx >= 1 && node.capture_idx <= n_captures as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn parse(pat: &str) -> (Box<Node>, Vec<Vec<u8>>, usize) {
        let mut p = Parser::new(pat.as_bytes(), 0);
        let root = p.parse().expect("parse failed");
        (root, p.names, p.n_captures)
    }

    #[test]
    fn decimal_backref_within_range_resolves_ok() {
        let (mut root, names, nc) = parse("(a)\\1");
        assert!(resolve_backrefs(&mut root, &names, nc));
        // Backref node still has capture_idx = 1 after resolve.
        let br = &root.kids[1];
        assert_eq!(br.kind, NodeKind::Backref);
        assert_eq!(br.capture_idx, 1);
    }

    #[test]
    fn decimal_backref_out_of_range_fails() {
        let (mut root, names, nc) = parse("(a)\\5");
        assert!(!resolve_backrefs(&mut root, &names, nc));
    }

    #[test]
    fn decimal_backref_forward_ok_when_in_range() {
        // \1 before (a) — forward ref; valid because n_captures known
        // post-parse.
        let (mut root, names, nc) = parse("\\1(a)");
        assert!(resolve_backrefs(&mut root, &names, nc));
    }

    #[test]
    fn named_backref_resolves_to_capture_idx() {
        let (mut root, names, nc) = parse("(?<x>a)\\k<x>");
        assert!(resolve_backrefs(&mut root, &names, nc));
        let br = &root.kids[1];
        assert_eq!(br.capture_idx, 1);
        assert!(br.backref_name.is_empty(), "name cleared after resolution");
    }

    #[test]
    fn named_backref_unknown_name_fails() {
        let (mut root, names, nc) = parse("(?<x>a)\\k<y>");
        assert!(!resolve_backrefs(&mut root, &names, nc));
    }

    #[test]
    fn named_backref_forward_ok() {
        let (mut root, names, nc) = parse("\\k<x>(?<x>a)");
        assert!(resolve_backrefs(&mut root, &names, nc));
    }

    #[test]
    fn nested_backref_walked() {
        // `((a)\1)` — backref inside an outer group.
        let (mut root, names, nc) = parse("((a)\\1)");
        assert!(resolve_backrefs(&mut root, &names, nc));
    }

    #[test]
    fn no_backrefs_trivially_passes() {
        let (mut root, names, nc) = parse("abc");
        assert!(resolve_backrefs(&mut root, &names, nc));
    }

    #[test]
    fn unresolved_backref_in_one_branch_fails_alt() {
        // \5 inside alt should still fail even though the other
        // branch is fine.
        let (mut root, names, nc) = parse("(a)(?:\\5|b)");
        assert!(!resolve_backrefs(&mut root, &names, nc));
    }
}
