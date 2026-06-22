//! NFA → DFA conversion substrate for backref-free patterns.
//!
//! Layered chunks:
//! - **Eligibility** (`analyze` + `DfaEligibility`) — pre-order AST
//!   walker reporting whether the pattern uses only DFA-representable
//!   opcodes. Result cached on `Program.can_dfa` at compile time.
//! - **ε-closure** (`epsilon_closure`) — given a `Program` and a seed
//!   PC list, return the set of PCs reachable via pure-ε transitions.
//!   Building block for subset construction.
//!
//! Future chunks: subset-construction NFA→DFA builder, per-RegExp
//! state cache, fast-path fork in `vm::search_from_with_ws`.
//! Tracking RFC: `.claude/rfcs/20260622-pike-vm-dfa/design.md`.

use alloc::collections::BTreeSet;
use alloc::vec::Vec;

use crate::node::{Node, NodeKind};
use crate::program::{Op, Program};

/// Outcome of DFA eligibility analysis.
///
/// `Eligible` means the pattern's AST uses only opcodes representable
/// in a classical Thompson → DFA subset construction. The blocker
/// variants name the specific feature that forces NFA simulation —
/// useful for future logging / heuristics ("how often do we lose DFA
/// fast path to backref vs lookaround?").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DfaEligibility {
    Eligible,
    HasBackref,
    HasLookahead,
    HasNegLookahead,
    HasLookbehind,
    HasNegLookbehind,
}

impl DfaEligibility {
    pub fn is_eligible(self) -> bool {
        matches!(self, DfaEligibility::Eligible)
    }
}

/// Walk `root` recursively, returning the first blocker encountered or
/// `Eligible` if the entire tree is DFA-compatible.
///
/// Order: self node → `child` (used by Repeat / Group / lookaround) →
/// `kids` (used by Concat / Alt). Pre-order so a blocker at the root is
/// reported without descending. Reentrant / stack depth follows the AST
/// nesting depth, which the parser already bounds via standard regex
/// grammar — no separate guard needed.
pub fn analyze(root: &Node) -> DfaEligibility {
    match root.kind {
        NodeKind::Backref => return DfaEligibility::HasBackref,
        NodeKind::Lookahead => return DfaEligibility::HasLookahead,
        NodeKind::NegLookahead => return DfaEligibility::HasNegLookahead,
        NodeKind::Lookbehind => return DfaEligibility::HasLookbehind,
        NodeKind::NegLookbehind => return DfaEligibility::HasNegLookbehind,
        _ => {}
    }
    if let Some(child) = root.child.as_ref() {
        let r = analyze(child);
        if !r.is_eligible() {
            return r;
        }
    }
    for kid in root.kids.iter() {
        let r = analyze(kid);
        if !r.is_eligible() {
            return r;
        }
    }
    DfaEligibility::Eligible
}

/// Compute the ε-closure of `seeds` under `prog` — the set of all PCs
/// reachable from any `seeds[i]` via pure-ε transitions.
///
/// ε ops handled in this chunk: [`Op::Jmp`] (unconditional), [`Op::Split`]
/// (both branches). Everything else (`Save`, the anchors, `WBound` /
/// `NWBound`, the lookaround variants, every byte-consuming op) is
/// terminal — its PC is recorded in the closure if it was reached, but
/// the walk does not follow its successor.
///
/// Save / Anchor / WBound are kept terminal here because they carry
/// state that a flat PC-set cannot represent: `Save` mutates a capture
/// slot (future chunk: track per-state save-mask), the anchors depend on
/// `pos` relative to text start / line boundaries (future chunk:
/// position-context DFA states tracking left-byte / at-start / at-end),
/// `WBound` / `NWBound` depend on the surrounding bytes. The subset
/// construction will revisit these once the per-state context is added.
///
/// PCs are deduped via the returned `BTreeSet`, so a circular `JMP`
/// chain terminates in O(n) where n = `prog.len()`. Out-of-range or
/// unknown-opcode PCs are silently dropped (defensive — mirrors
/// `vm/dispatch.rs::add_thread`).
pub fn epsilon_closure(prog: &Program, seeds: &[usize]) -> BTreeSet<usize> {
    let mut closure: BTreeSet<usize> = BTreeSet::new();
    let mut work: Vec<usize> = Vec::new();
    for &seed in seeds {
        if seed < prog.len() && closure.insert(seed) {
            work.push(seed);
        }
    }
    while let Some(pc) = work.pop() {
        let ins = prog.insts[pc];
        let op = match Op::from_u8(ins.op) {
            Some(o) => o,
            None => continue,
        };
        match op {
            Op::Jmp => {
                let t = ins.a as usize;
                if t < prog.len() && closure.insert(t) {
                    work.push(t);
                }
            }
            Op::Split => {
                let t1 = ins.a as usize;
                let t2 = ins.b as usize;
                if t1 < prog.len() && closure.insert(t1) {
                    work.push(t1);
                }
                if t2 < prog.len() && closure.insert(t2) {
                    work.push(t2);
                }
            }
            _ => {}
        }
    }
    closure
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn char_node(b: u8) -> alloc::boxed::Box<Node> {
        let mut n = Node::new(NodeKind::Char);
        n.ch = b;
        n
    }

    #[test]
    fn single_char_is_eligible() {
        let n = char_node(b'a');
        assert_eq!(analyze(&n), DfaEligibility::Eligible);
        assert!(analyze(&n).is_eligible());
    }

    #[test]
    fn anychar_class_anchors_are_eligible() {
        for k in [
            NodeKind::Any,
            NodeKind::Class,
            NodeKind::AnchorBeg,
            NodeKind::AnchorEnd,
            NodeKind::WBound,
            NodeKind::NWBound,
        ] {
            let n = Node::new(k);
            assert_eq!(analyze(&n), DfaEligibility::Eligible, "{k:?}");
        }
    }

    #[test]
    fn concat_of_chars_is_eligible() {
        let mut concat = Node::new(NodeKind::Concat);
        concat.push_kid(char_node(b'a'));
        concat.push_kid(char_node(b'b'));
        concat.push_kid(char_node(b'c'));
        assert_eq!(analyze(&concat), DfaEligibility::Eligible);
    }

    #[test]
    fn alt_of_concats_is_eligible() {
        let mut alt = Node::new(NodeKind::Alt);
        for c in [b'a', b'b'] {
            let mut concat = Node::new(NodeKind::Concat);
            concat.push_kid(char_node(c));
            alt.push_kid(concat);
        }
        assert_eq!(analyze(&alt), DfaEligibility::Eligible);
    }

    #[test]
    fn repeat_over_class_is_eligible() {
        let mut rep = Node::new(NodeKind::Repeat);
        rep.min = 0;
        rep.max = -1;
        rep.child = Some(Node::new(NodeKind::Class));
        assert_eq!(analyze(&rep), DfaEligibility::Eligible);
    }

    #[test]
    fn group_around_char_is_eligible() {
        let mut g = Node::new(NodeKind::Group);
        g.capture_idx = 1;
        g.child = Some(char_node(b'x'));
        assert_eq!(analyze(&g), DfaEligibility::Eligible);
    }

    #[test]
    fn root_backref_is_blocker() {
        let n = Node::new(NodeKind::Backref);
        assert_eq!(analyze(&n), DfaEligibility::HasBackref);
        assert!(!analyze(&n).is_eligible());
    }

    #[test]
    fn nested_backref_is_blocker() {
        let mut concat = Node::new(NodeKind::Concat);
        concat.push_kid(char_node(b'a'));
        concat.push_kid(Node::new(NodeKind::Backref));
        assert_eq!(analyze(&concat), DfaEligibility::HasBackref);
    }

    #[test]
    fn lookahead_variants_are_blockers() {
        for (k, want) in [
            (NodeKind::Lookahead, DfaEligibility::HasLookahead),
            (NodeKind::NegLookahead, DfaEligibility::HasNegLookahead),
            (NodeKind::Lookbehind, DfaEligibility::HasLookbehind),
            (NodeKind::NegLookbehind, DfaEligibility::HasNegLookbehind),
        ] {
            let mut n = Node::new(k);
            n.child = Some(char_node(b'x'));
            assert_eq!(analyze(&n), want, "{k:?}");
        }
    }

    #[test]
    fn lookahead_deeply_nested_in_group_is_blocker() {
        let mut outer = Node::new(NodeKind::Group);
        outer.capture_idx = 1;
        let mut rep = Node::new(NodeKind::Repeat);
        rep.min = 1;
        rep.max = -1;
        let mut la = Node::new(NodeKind::Lookahead);
        la.child = Some(char_node(b'z'));
        rep.child = Some(la);
        outer.child = Some(rep);
        assert_eq!(analyze(&outer), DfaEligibility::HasLookahead);
    }

    #[test]
    fn first_blocker_wins_pre_order() {
        let mut alt = Node::new(NodeKind::Alt);
        alt.push_kid(Node::new(NodeKind::Backref));
        alt.push_kid(Node::new(NodeKind::Lookahead));
        assert_eq!(analyze(&alt), DfaEligibility::HasBackref);
    }

    #[test]
    fn empty_concat_is_eligible() {
        let n = Node::new(NodeKind::Concat);
        assert_eq!(analyze(&n), DfaEligibility::Eligible);
    }

    // Integration: parse real regex source through the parser pipeline
    // (including `resolve_backrefs` for named `\k<name>`) and check the
    // eligibility verdict against a curated truth table.
    fn parse_and_analyze(pattern: &[u8], flags: u8) -> DfaEligibility {
        use crate::parser::Parser;
        use crate::resolve::resolve_backrefs;
        let mut parser = Parser::new(pattern, flags);
        let mut root = parser.parse().expect("parse should succeed");
        let names = parser.names.clone();
        let n_captures = parser.n_captures;
        assert!(
            resolve_backrefs(&mut root, &names, n_captures),
            "resolve_backrefs should succeed for {pattern:?}"
        );
        analyze(&root)
    }

    #[test]
    fn parsed_literal_pattern_is_eligible() {
        assert_eq!(parse_and_analyze(b"abc", 0), DfaEligibility::Eligible);
    }

    #[test]
    fn parsed_alt_repeat_class_pattern_is_eligible() {
        assert_eq!(
            parse_and_analyze(b"(foo|bar)+[0-9]*\\b", 0),
            DfaEligibility::Eligible
        );
    }

    #[test]
    fn parsed_anchor_dotall_pattern_is_eligible() {
        assert_eq!(parse_and_analyze(b"^.*$", 0), DfaEligibility::Eligible);
    }

    #[test]
    fn parsed_backref_pattern_is_blocked() {
        // `(a)\1` — capture + decimal backref to it.
        assert_eq!(parse_and_analyze(b"(a)\\1", 0), DfaEligibility::HasBackref);
    }

    #[test]
    fn parsed_named_backref_pattern_is_blocked() {
        // `(?<x>a)\k<x>` — named capture + `\k<name>` backref.
        // `resolve_backrefs` rewrites this into a `Backref` node with
        // the resolved capture index; without it the walker would not
        // see the blocker.
        assert_eq!(
            parse_and_analyze(b"(?<x>a)\\k<x>", 0),
            DfaEligibility::HasBackref
        );
    }

    #[test]
    fn parsed_lookahead_pattern_is_blocked() {
        assert_eq!(
            parse_and_analyze(b"foo(?=bar)", 0),
            DfaEligibility::HasLookahead
        );
    }

    #[test]
    fn parsed_neg_lookahead_pattern_is_blocked() {
        assert_eq!(
            parse_and_analyze(b"foo(?!bar)", 0),
            DfaEligibility::HasNegLookahead
        );
    }

    #[test]
    fn parsed_lookbehind_pattern_is_blocked() {
        assert_eq!(
            parse_and_analyze(b"(?<=foo)bar", 0),
            DfaEligibility::HasLookbehind
        );
    }

    #[test]
    fn parsed_neg_lookbehind_pattern_is_blocked() {
        assert_eq!(
            parse_and_analyze(b"(?<!foo)bar", 0),
            DfaEligibility::HasNegLookbehind
        );
    }

    // ε-closure tests — synthetic mini-Programs that exercise the JMP /
    // SPLIT walk plus the "terminal" treatment of every other op.

    use crate::program::Inst;

    fn into_set(v: &[usize]) -> BTreeSet<usize> {
        v.iter().copied().collect()
    }

    #[test]
    fn epsilon_closure_of_empty_seed_list_is_empty() {
        let prog = Program::new();
        assert!(epsilon_closure(&prog, &[]).is_empty());
    }

    #[test]
    fn seed_out_of_range_is_dropped() {
        let prog = Program::new();
        assert!(epsilon_closure(&prog, &[0, 999]).is_empty());
    }

    #[test]
    fn seed_at_char_op_only_contains_self() {
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0]));
    }

    #[test]
    fn jmp_chain_walks_to_terminal_op() {
        // 0: JMP 2 → 2: JMP 4 → 4: CHAR a → 5: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::jmp(2));
        prog.emit(Inst::char_lit(b'x')); // unreachable filler
        prog.emit(Inst::jmp(4));
        prog.emit(Inst::char_lit(b'y')); // unreachable filler
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0, 2, 4]));
    }

    #[test]
    fn split_forks_both_targets() {
        // 0: SPLIT 1, 3 — both targets are char ops (terminal)
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 3));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept()); // unreachable filler
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0, 1, 3]));
    }

    #[test]
    fn split_to_split_walks_transitively() {
        // 0: SPLIT 1,2; 1: SPLIT 3,4; 2: char; 3: char; 4: char
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 2));
        prog.emit(Inst::split(3, 4));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::char_lit(b'c'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0, 1, 2, 3, 4]));
    }

    #[test]
    fn circular_jmp_terminates() {
        // 0: JMP 1; 1: JMP 0 — must not loop forever
        let mut prog = Program::new();
        prog.emit(Inst::jmp(1));
        prog.emit(Inst::jmp(0));
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0, 1]));
    }

    #[test]
    fn save_op_is_terminal_in_this_chunk() {
        // 0: SAVE 0; 1: CHAR a — closure of {0} = {0} only (not {0, 1});
        // SAVE walk is deferred to a future save-mask-aware chunk.
        let mut prog = Program::new();
        prog.emit(Inst::save(0));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0]));
    }

    #[test]
    fn anchor_b_is_terminal_in_this_chunk() {
        // 0: ANCHOR_B; 1: CHAR a — closure of {0} = {0}; anchor walk
        // deferred to a future position-context-aware chunk.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0]));
    }

    #[test]
    fn multi_seed_unions_per_seed_closures() {
        // 0: JMP 2; 2: CHAR a; 3: JMP 5; 5: CHAR b
        let mut prog = Program::new();
        prog.emit(Inst::jmp(2));
        prog.emit(Inst::char_lit(b'x')); // filler
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::jmp(5));
        prog.emit(Inst::char_lit(b'y')); // filler
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0, 3]), into_set(&[0, 2, 3, 5]));
    }
}
