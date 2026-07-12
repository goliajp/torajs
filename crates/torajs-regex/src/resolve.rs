//! Post-parse backref resolution — port of `runtime_regex.c`
//! L1411-1439.
//!
//! Two backref forms land in the AST as [`NodeKind::Backref`]:
//!
//! - **Decimal** `\N..` — parser greedy-reads the digit run into
//!   `capture_idx` and keeps the raw digits in `backref_name`.
//!   Validated post-parse against `n_captures` (forward references
//!   are fine). Out-of-range refs demote per ECMA Annex B §B.1.4
//!   (chunk 804): without the `u` flag the digit run reinterprets
//!   as a LegacyOctalEscapeSequence (longest octal prefix ≤ 0377,
//!   first digit 0-3 up to three digits / 4-7 up to two) followed
//!   by literal digit chars; `\8` / `\9` are identity escapes.
//!   With `u` the out-of-range ref stays an error (spec
//!   SyntaxError → the rejected path).
//! - **Named** `\k<name>` — parser sets `capture_idx = -1` and
//!   stores the name bytes in `Node.backref_name`. Resolution looks
//!   up the name in the parser's name table to find the matching
//!   capture index.
//!
//! Returns `true` on success (all backrefs validated / resolved /
//! demoted), `false` on the first unresolved reference (named ref
//! to unknown name, or out-of-range positional under `u`).

use crate::node::{Node, NodeKind};
use crate::parser::RE_FLAG_U;
use crate::utf8::utf8_encode_cp;
use alloc::vec::Vec;

/// Walk `node` recursively, validating + resolving every Backref.
/// `names` is indexed by capture_idx 1..=n_captures (slot 0 unused);
/// an empty `Vec<u8>` at a slot means that capture has no name.
pub fn resolve_backrefs(node: &mut Node, names: &[Vec<u8>], n_captures: usize, flags: u8) -> bool {
    if node.kind == NodeKind::Backref && !resolve_one(node, names, n_captures, flags) {
        return false;
    }
    if let Some(child) = node.child.as_deref_mut()
        && !resolve_backrefs(child, names, n_captures, flags)
    {
        return false;
    }
    for kid in &mut node.kids {
        if !resolve_backrefs(kid, names, n_captures, flags) {
            return false;
        }
    }
    true
}

fn resolve_one(node: &mut Node, names: &[Vec<u8>], n_captures: usize, flags: u8) -> bool {
    if node.capture_idx == -1 && !node.backref_name.is_empty() {
        for i in 1..=n_captures {
            if names.get(i).is_some_and(|n| n == &node.backref_name) {
                node.capture_idx = i as i32;
                node.backref_name.clear();
                return true;
            }
        }
        false
    } else if node.capture_idx >= 1 && node.capture_idx <= n_captures as i32 {
        node.backref_name.clear();
        true
    } else if flags & RE_FLAG_U == 0 {
        demote_annexb(node);
        true
    } else {
        false
    }
}

/// Annex B §B.1.4 — rewrite an out-of-range decimal backref in
/// place as its LegacyOctalEscapeSequence / IdentityEscape reading:
/// the longest octal prefix of the digit run becomes one code point
/// (UTF-8 encoded like `\uHHHH`), every remaining digit a literal
/// char. `\8` / `\9` have no octal prefix, so the whole run is
/// literal digits.
fn demote_annexb(node: &mut Node) {
    let digits = core::mem::take(&mut node.backref_name);
    node.capture_idx = -1;
    let d0 = digits[0] - b'0';
    let mut bytes: Vec<u8> = Vec::new();
    let mut rest_start = 0;
    if d0 <= 7 {
        // Longest legacy octal prefix: first digit 0-3 → up to 3
        // digits, 4-7 → up to 2; stop at the first non-octal digit.
        let max_len = if d0 <= 3 { 3 } else { 2 };
        let mut val: u32 = 0;
        let mut k = 0;
        while k < digits.len().min(max_len) {
            let d = digits[k] - b'0';
            if d >= 8 {
                break;
            }
            val = val * 8 + u32::from(d);
            k += 1;
        }
        let mut buf = [0u8; 4];
        let blen = utf8_encode_cp(val as i32, &mut buf);
        bytes.extend_from_slice(&buf[..blen]);
        rest_start = k;
    }
    bytes.extend_from_slice(&digits[rest_start..]);
    if bytes.len() == 1 {
        node.kind = NodeKind::Char;
        node.ch = bytes[0];
        return;
    }
    node.kind = NodeKind::Concat;
    for &b in &bytes {
        let mut kid = Node::new(NodeKind::Char);
        kid.ch = b;
        // Synthesized literals inherit the demoted backref's effective
        // i/m/s scope (regexp-modifiers) — `(?i:(\12))`-style bodies
        // keep case-insensitivity on the demoted chars.
        kid.eff_ims = node.eff_ims;
        node.kids.push(kid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use alloc::boxed::Box;

    fn parse(pat: &str) -> (Box<Node>, Vec<Vec<u8>>, usize) {
        let mut p = Parser::new(pat.as_bytes(), 0);
        let root = p.parse().expect("parse failed");
        (root, p.names, p.n_captures)
    }

    #[test]
    fn decimal_backref_within_range_resolves_ok() {
        let (mut root, names, nc) = parse("(a)\\1");
        assert!(resolve_backrefs(&mut root, &names, nc, 0));
        // Backref node still has capture_idx = 1 after resolve.
        let br = &root.kids[1];
        assert_eq!(br.kind, NodeKind::Backref);
        assert_eq!(br.capture_idx, 1);
    }

    #[test]
    fn decimal_backref_out_of_range_demotes_to_octal() {
        // Annex B: `\5` with one group is octal 5 → literal \x05.
        let (mut root, names, nc) = parse("(a)\\5");
        assert!(resolve_backrefs(&mut root, &names, nc, 0));
        let n = &root.kids[1];
        assert_eq!(n.kind, NodeKind::Char);
        assert_eq!(n.ch, 5);
    }

    #[test]
    fn decimal_backref_out_of_range_fails_under_u() {
        let (mut root, names, nc) = parse("(a)\\5");
        assert!(!resolve_backrefs(&mut root, &names, nc, RE_FLAG_U));
    }

    #[test]
    fn octal_demotion_splits_prefix_and_literals() {
        // `\1234` with no groups → octal 123 (= 'S') + literal '4'.
        let (mut root, names, nc) = parse("\\1234");
        assert!(resolve_backrefs(&mut root, &names, nc, 0));
        let n = &root.kids[0];
        assert_eq!(n.kind, NodeKind::Concat);
        assert_eq!(n.kids.len(), 2);
        assert_eq!(n.kids[0].ch, 0o123);
        assert_eq!(n.kids[1].ch, b'4');
    }

    #[test]
    fn four_to_seven_octal_prefix_is_two_digits() {
        // `\777` → octal 77 (= '?') + literal '7'.
        let (mut root, names, nc) = parse("\\777");
        assert!(resolve_backrefs(&mut root, &names, nc, 0));
        let n = &root.kids[0];
        assert_eq!(n.kind, NodeKind::Concat);
        assert_eq!(n.kids[0].ch, 0o77);
        assert_eq!(n.kids[1].ch, b'7');
    }

    #[test]
    fn eight_nine_demote_to_identity() {
        let (mut root, names, nc) = parse("(a)\\9");
        assert!(resolve_backrefs(&mut root, &names, nc, 0));
        let n = &root.kids[1];
        assert_eq!(n.kind, NodeKind::Char);
        assert_eq!(n.ch, b'9');
    }

    #[test]
    fn decimal_backref_forward_ok_when_in_range() {
        // \1 before (a) — forward ref; valid because n_captures known
        // post-parse.
        let (mut root, names, nc) = parse("\\1(a)");
        assert!(resolve_backrefs(&mut root, &names, nc, 0));
    }

    #[test]
    fn named_backref_resolves_to_capture_idx() {
        let (mut root, names, nc) = parse("(?<x>a)\\k<x>");
        assert!(resolve_backrefs(&mut root, &names, nc, 0));
        let br = &root.kids[1];
        assert_eq!(br.capture_idx, 1);
        assert!(br.backref_name.is_empty(), "name cleared after resolution");
    }

    #[test]
    fn named_backref_unknown_name_fails() {
        let (mut root, names, nc) = parse("(?<x>a)\\k<y>");
        assert!(!resolve_backrefs(&mut root, &names, nc, 0));
    }

    #[test]
    fn named_backref_forward_ok() {
        let (mut root, names, nc) = parse("\\k<x>(?<x>a)");
        assert!(resolve_backrefs(&mut root, &names, nc, 0));
    }

    #[test]
    fn nested_backref_walked() {
        // `((a)\1)` — backref inside an outer group.
        let (mut root, names, nc) = parse("((a)\\1)");
        assert!(resolve_backrefs(&mut root, &names, nc, 0));
    }

    #[test]
    fn no_backrefs_trivially_passes() {
        let (mut root, names, nc) = parse("abc");
        assert!(resolve_backrefs(&mut root, &names, nc, 0));
    }

    #[test]
    fn out_of_range_in_alt_branch_demotes() {
        // `\5` inside alt demotes to octal under Annex B (used to
        // fail resolve pre-804).
        let (mut root, names, nc) = parse("(a)(?:\\5|b)");
        assert!(resolve_backrefs(&mut root, &names, nc, 0));
    }
}
