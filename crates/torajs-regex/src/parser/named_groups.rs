//! Named-capture helpers — pre-scan for annexB `\k` gating
//! (`scan_has_named_groups`) and the ES §22.2.1.1 Static Semantics
//! duplicate-name Alternative constraint
//! (`check_named_groups_disjunction_ok`).

use crate::node::{Node, NodeKind};
use alloc::vec::Vec;

/// Pre-scan the pattern bytes for any `(?<name>...)` named capture
/// group. Skips escape sequences (`\X` as one atom) and character
/// class bodies (`[...]`) since a `(?<` there is not a group
/// opener. Distinguishes named-group `(?<X` (where X is not `=` /
/// `!`) from lookbehind assertions `(?<=` / `(?<!`.
///
/// Used to gate the annexB §B.1.4 `\k<name>` identity fallback:
/// only patterns with zero named groups + non-u/v mode may take
/// the fallback.
pub(super) fn scan_has_named_groups(p: &[u8]) -> bool {
    let mut i = 0;
    let mut in_class = false;
    while i < p.len() {
        let c = p[i];
        if c == b'\\' && i + 1 < p.len() {
            i += 2;
            continue;
        }
        if !in_class && c == b'[' {
            in_class = true;
            i += 1;
            continue;
        }
        if in_class {
            if c == b']' {
                in_class = false;
            }
            i += 1;
            continue;
        }
        if c == b'(' && i + 2 < p.len() && p[i + 1] == b'?' && p[i + 2] == b'<' {
            let next = if i + 3 < p.len() { p[i + 3] } else { 0 };
            if next != b'=' && next != b'!' {
                return true;
            }
            i += 3;
            continue;
        }
        i += 1;
    }
    false
}

/// ES §22.2.1.1 Static Semantics: Early Errors — "It is a Syntax
/// Error if any two GroupNames in this Pattern have the same
/// StringValue and the two named-capturing groups are contained
/// within the same Alternative." The ES2025 Duplicate Named
/// Capture Groups proposal carves out disjunction siblings:
/// `(?<x>a)|(?<x>b)` is legal because the two `<x>` are in
/// *different* Alternatives, but `(?<x>a)(?<x>b)` is not.
///
/// Two named groups are *mutually exclusive* (legal dup) iff their
/// paths from the root diverge at some `Alt` node with different
/// branch indices — descend into different children of a common
/// `Alt` ancestor. If one path is a prefix of the other (equal, or
/// one lives inside a nested Alternative of the other's
/// Alternative), they can both match simultaneously → dup-name
/// conflict → SyntaxError.
///
/// Returns `true` if the pattern is well-formed (no dup-name
/// conflict), `false` if any dup pair is non-mutually-exclusive.
pub(super) fn check_named_groups_disjunction_ok(root: &Node, names: &[Vec<u8>]) -> bool {
    // Fast path: at most one named group → no possible dup.
    let named_count = names.iter().filter(|n| !n.is_empty()).count();
    if named_count <= 1 {
        return true;
    }
    let mut entries: Vec<(usize, Vec<(u32, u32)>)> = Vec::new();
    let mut alt_id_counter: u32 = 0;
    let mut fingerprint: Vec<(u32, u32)> = Vec::new();
    collect_named_captures(root, &mut fingerprint, &mut alt_id_counter, &mut entries);
    for i in 0..entries.len() {
        for j in (i + 1)..entries.len() {
            let (ci, fi) = &entries[i];
            let (cj, fj) = &entries[j];
            if names[*ci] == names[*cj] && !fingerprints_diverge(fi, fj) {
                return false;
            }
        }
    }
    true
}

fn collect_named_captures(
    node: &Node,
    fp: &mut Vec<(u32, u32)>,
    alt_id: &mut u32,
    out: &mut Vec<(usize, Vec<(u32, u32)>)>,
) {
    if node.kind == NodeKind::Group && node.capture_idx > 0 {
        // capture_idx>0 doesn't imply named — unnamed groups still
        // have positive idx. Caller filters by names[ci] non-empty
        // during the pair scan, so we record all positive-idx
        // groups here and let the O(n²) pass skip unnamed ones.
        out.push((node.capture_idx as usize, fp.clone()));
    }
    match node.kind {
        NodeKind::Alt => {
            let this_alt = *alt_id;
            *alt_id += 1;
            for (branch_idx, kid) in node.kids.iter().enumerate() {
                fp.push((this_alt, branch_idx as u32));
                collect_named_captures(kid, fp, alt_id, out);
                fp.pop();
            }
        }
        NodeKind::Concat => {
            for kid in &node.kids {
                collect_named_captures(kid, fp, alt_id, out);
            }
        }
        NodeKind::Group
        | NodeKind::Lookahead
        | NodeKind::NegLookahead
        | NodeKind::Lookbehind
        | NodeKind::NegLookbehind
        | NodeKind::Repeat => {
            if let Some(child) = &node.child {
                collect_named_captures(child, fp, alt_id, out);
            }
        }
        _ => {}
    }
}

/// Two fingerprints "diverge" iff neither is a prefix of the
/// other — they take different branches at some common `Alt`
/// ancestor. That makes the two captures mutually exclusive.
fn fingerprints_diverge(a: &[(u32, u32)], b: &[(u32, u32)]) -> bool {
    let min_len = a.len().min(b.len());
    for k in 0..min_len {
        if a[k] != b[k] {
            // Same alt_id (identical prefix walk) with different
            // branch → mutually exclusive.
            return true;
        }
    }
    // One is prefix of the other → simultaneous → conflict.
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn ok(pattern: &str) -> bool {
        let mut p = Parser::new(pattern.as_bytes(), 0);
        p.parse().is_some() && !p.err()
    }

    #[test]
    fn single_named_group_ok() {
        assert!(ok("(?<x>a)"));
    }

    #[test]
    fn distinct_names_ok() {
        assert!(ok("(?<x>a)(?<y>b)"));
    }

    #[test]
    fn dup_concat_rejected() {
        assert!(!ok("(?<x>a)(?<x>b)"));
    }

    #[test]
    fn dup_top_level_disjunction_ok() {
        assert!(ok("(?<x>a)|(?<x>b)"));
    }

    #[test]
    fn dup_grouped_disjunction_ok() {
        assert!(ok("(?:(?<x>a)|(?<x>b))"));
    }

    #[test]
    fn dup_across_disjunction_and_outer_concat_rejected() {
        // Outer `<x>` and inner disjunction-sibling `<x>` both live
        // in the same top-level Alternative → not mutually excl.
        assert!(!ok("(?<x>a)(?:(?<x>b)|c)"));
    }

    #[test]
    fn dup_after_disjunction_rejected() {
        // Third `<x>` concurrent with each inner branch's `<x>`.
        assert!(!ok("((?<x>a)|(?<x>b))(?<x>c)"));
    }

    #[test]
    fn dup_nested_disjunction_ok() {
        // Top-level branch 0 has one `<x>`; branch 1 has another.
        // Both live in different top-level alt branches.
        assert!(ok("(?<x>a)|(?:(?<x>b)|c)"));
    }

    #[test]
    fn scan_finds_named_group() {
        assert!(scan_has_named_groups(b"(?<x>a)"));
        assert!(!scan_has_named_groups(b"(?:a)"));
        assert!(!scan_has_named_groups(b"(?<=a)b"));
        assert!(!scan_has_named_groups(b"[(?<x>]"));
    }
}
