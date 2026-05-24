//! Thompson NFA construction — port of `runtime_regex.c`
//! L1157-1350 (`compile_node` / `compile_repeat` / `compile_alt`).
//!
//! Walks an AST produced by [`crate::parser`] and emits flat
//! bytecode into a [`Program`]. Backpatching uses the index returned
//! by `Program::emit` + the `next_idx` cursor; jump targets are
//! finalized once the relevant sub-tree is in place.

use crate::node::{Node, NodeKind};
use crate::program::{Inst, Op, Program};

/// Compile `node` into `prog`. The emitted bytecode is appended; the
/// caller is responsible for the outer `OP_MATCH` once the root of the
/// whole pattern has been compiled.
pub fn compile(prog: &mut Program, node: &Node) {
    match node.kind {
        NodeKind::Char => {
            prog.emit(Inst::char_lit(node.ch));
        }
        NodeKind::Any => {
            prog.emit(Inst::simple(Op::AnyChar));
        }
        NodeKind::Class => {
            let cidx = prog.intern_class(&node.cc);
            prog.emit(Inst::class_ref(cidx));
        }
        NodeKind::AnchorBeg => {
            prog.emit(Inst::simple(Op::AnchorB));
        }
        NodeKind::AnchorEnd => {
            prog.emit(Inst::simple(Op::AnchorE));
        }
        NodeKind::WBound => {
            prog.emit(Inst::simple(Op::WBound));
        }
        NodeKind::NWBound => {
            prog.emit(Inst::simple(Op::NWBound));
        }
        NodeKind::Concat => {
            for kid in &node.kids {
                compile(prog, kid);
            }
        }
        NodeKind::Alt => compile_alt(prog, node),
        NodeKind::Repeat => compile_repeat(prog, node),
        NodeKind::Group => compile_group(prog, node),
        NodeKind::Backref => {
            prog.emit(Inst::backref(node.capture_idx));
        }
        NodeKind::Lookahead
        | NodeKind::NegLookahead
        | NodeKind::Lookbehind
        | NodeKind::NegLookbehind => compile_lookaround(prog, node),
    }
}

/// `a | b | c | ...` lowers to:
///
/// ```text
///   SPLIT L1, Lalt
///   L1:    compile(a); JMP Lend
///   Lalt:  SPLIT L2, Lalt2
///   L2:    compile(b); JMP Lend
///   Lalt2: compile(c)
///   Lend:
/// ```
fn compile_alt(prog: &mut Program, node: &Node) {
    let n_alts = node.kids.len();
    if n_alts == 0 {
        return; // defensive — parser doesn't produce empty Alt
    }
    let mut jmps = Vec::with_capacity(n_alts);
    for kid in &node.kids[..n_alts - 1] {
        let sidx = prog.emit(Inst::split(0, 0));
        let branch_start = prog.next_idx();
        compile(prog, kid);
        let jmp_idx = prog.emit(Inst::jmp(0));
        jmps.push(jmp_idx as usize);
        let next = prog.next_idx();
        prog.insts[sidx as usize].a = branch_start;
        prog.insts[sidx as usize].b = next;
    }
    // Last alternative — no trailing JMP; falls through to Lend.
    compile(prog, &node.kids[n_alts - 1]);
    let end = prog.next_idx();
    for jidx in jmps {
        prog.insts[jidx].a = end;
    }
}

/// `{min, max}` lowers to:
///
/// - `min` unrolled mandatory copies of `child`.
/// - For unbounded (`max == -1`), a SPLIT-loop Kleene star tail.
/// - For bounded (`max - min` extras), a chain of `SPLIT (body, skip)`
///   wrappers — each loop iteration may exit early via `skip`.
fn compile_repeat(prog: &mut Program, node: &Node) {
    let Some(child) = node.child.as_deref() else {
        return;
    };
    // Unrolled mandatory prefix.
    for _ in 0..node.min {
        compile(prog, child);
    }
    if node.max == -1 {
        compile_kleene_tail(prog, child, node.lazy);
    } else {
        let extra = node.max - node.min;
        compile_bounded_extras(prog, child, extra, node.lazy);
    }
}

/// SPLIT-loop tail for unbounded repeats (`*` / `+` / `{n,}`).
/// Greedy: `SPLIT body, after`; lazy: targets swapped.
fn compile_kleene_tail(prog: &mut Program, child: &Node, lazy: bool) {
    let split_idx = prog.emit(Inst::split(0, 0));
    let body_start = prog.next_idx();
    compile(prog, child);
    prog.emit(Inst::jmp(split_idx));
    let after = prog.next_idx();
    if lazy {
        prog.insts[split_idx as usize].a = after;
        prog.insts[split_idx as usize].b = body_start;
    } else {
        prog.insts[split_idx as usize].a = body_start;
        prog.insts[split_idx as usize].b = after;
    }
}

/// `extra` bounded optional iterations of `child`, each wrapped in a
/// SPLIT that can fall through to `after_loop`. Backpatched once the
/// extras are emitted.
fn compile_bounded_extras(prog: &mut Program, child: &Node, extra: i32, lazy: bool) {
    if extra <= 0 {
        return;
    }
    let mut splits = Vec::with_capacity(extra as usize);
    for _ in 0..extra {
        let sidx = prog.emit(Inst::split(0, 0));
        splits.push(sidx as usize);
        let body_start = prog.next_idx();
        compile(prog, child);
        if lazy {
            prog.insts[sidx as usize].a = -1; // skip — patched below
            prog.insts[sidx as usize].b = body_start;
        } else {
            prog.insts[sidx as usize].a = body_start;
            prog.insts[sidx as usize].b = -1; // skip — patched below
        }
    }
    let after = prog.next_idx();
    for sidx in splits {
        if prog.insts[sidx].a == -1 {
            prog.insts[sidx].a = after;
        }
        if prog.insts[sidx].b == -1 {
            prog.insts[sidx].b = after;
        }
    }
}

/// `(...)` or `(?:...)`. Capturing groups bracket the child with two
/// `SAVE` instructions writing `pos` to slots `2*idx` and `2*idx+1`.
fn compile_group(prog: &mut Program, node: &Node) {
    let Some(child) = node.child.as_deref() else {
        return;
    };
    if node.capture_idx > 0 {
        prog.emit(Inst::save(2 * node.capture_idx));
        compile(prog, child);
        prog.emit(Inst::save(2 * node.capture_idx + 1));
    } else {
        compile(prog, child);
    }
}

/// `(?=...)` / `(?!...)` / `(?<=...)` / `(?<!...)`. The body compiles
/// into a fresh sub-Program (with its own `OP_MATCH` terminator); the
/// parent emits the appropriate `OP_*_LOOKAHEAD/BEHIND` op pointing at
/// the sub-Program's index.
fn compile_lookaround(prog: &mut Program, node: &Node) {
    let mut sub = Program::new();
    if let Some(child) = node.child.as_deref() {
        compile(&mut sub, child);
    }
    sub.emit(Inst::match_accept());
    let sub_idx = prog.add_sub(Box::new(sub));
    let op = match node.kind {
        NodeKind::Lookahead => Op::Lookahead,
        NodeKind::NegLookahead => Op::NegLookahead,
        NodeKind::Lookbehind => Op::Lookbehind,
        NodeKind::NegLookbehind => Op::NegLookbehind,
        _ => unreachable!("compile_lookaround called with non-lookaround kind"),
    };
    prog.emit(Inst::lookaround(op, sub_idx));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn compile_pattern(pat: &str) -> Program {
        let mut p = Parser::new(pat.as_bytes(), 0);
        let root = p.parse().expect("parse failed");
        let mut prog = Program::new();
        compile(&mut prog, &root);
        prog.emit(Inst::match_accept());
        prog
    }

    fn ops(prog: &Program) -> Vec<Op> {
        prog.insts
            .iter()
            .map(|i| Op::from_u8(i.op).unwrap())
            .collect()
    }

    #[test]
    fn literal_char_emits_char_then_match() {
        let prog = compile_pattern("a");
        assert_eq!(ops(&prog), vec![Op::Char, Op::Match]);
        assert_eq!(prog.insts[0].ch, b'a');
    }

    #[test]
    fn concat_emits_sequence() {
        let prog = compile_pattern("abc");
        assert_eq!(ops(&prog), vec![Op::Char, Op::Char, Op::Char, Op::Match]);
    }

    #[test]
    fn dot_emits_any_char() {
        let prog = compile_pattern(".");
        assert_eq!(prog.insts[0].op, Op::AnyChar as u8);
    }

    #[test]
    fn class_emits_op_class_with_interned_idx() {
        let prog = compile_pattern("\\d");
        assert_eq!(prog.insts[0].op, Op::Class as u8);
        assert_eq!(prog.insts[0].a, 0);
        assert_eq!(prog.classes.len(), 1);
    }

    #[test]
    fn alternation_emits_split_jmp_chain() {
        let prog = compile_pattern("a|b");
        // SPLIT, CHAR a, JMP, CHAR b, MATCH
        let o = ops(&prog);
        assert_eq!(o.len(), 5);
        assert_eq!(o[0], Op::Split);
        assert_eq!(o[1], Op::Char);
        assert_eq!(o[2], Op::Jmp);
        assert_eq!(o[3], Op::Char);
        assert_eq!(o[4], Op::Match);
        assert_eq!(prog.insts[0].a, 1); // SPLIT.a → CHAR a
        assert_eq!(prog.insts[0].b, 3); // SPLIT.b → CHAR b
        assert_eq!(prog.insts[2].a, 4); // JMP → MATCH (after Lend)
    }

    #[test]
    fn star_emits_kleene_tail_greedy() {
        let prog = compile_pattern("a*");
        // SPLIT, CHAR a, JMP, MATCH
        let o = ops(&prog);
        assert_eq!(o, vec![Op::Split, Op::Char, Op::Jmp, Op::Match]);
        assert_eq!(prog.insts[0].a, 1); // body
        assert_eq!(prog.insts[0].b, 3); // after (skip)
        assert_eq!(prog.insts[2].a, 0); // JMP → SPLIT
    }

    #[test]
    fn star_lazy_swaps_split_targets() {
        let prog = compile_pattern("a*?");
        assert_eq!(prog.insts[0].a, 3); // after (skip first)
        assert_eq!(prog.insts[0].b, 1); // body
    }

    #[test]
    fn plus_emits_mandatory_then_kleene() {
        let prog = compile_pattern("a+");
        // CHAR a (mandatory), SPLIT, CHAR a, JMP, MATCH
        let o = ops(&prog);
        assert_eq!(o, vec![Op::Char, Op::Split, Op::Char, Op::Jmp, Op::Match]);
    }

    #[test]
    fn question_emits_single_split() {
        let prog = compile_pattern("a?");
        // SPLIT, CHAR a, MATCH
        let o = ops(&prog);
        assert_eq!(o, vec![Op::Split, Op::Char, Op::Match]);
    }

    #[test]
    fn braced_exact_emits_unrolled_copies() {
        let prog = compile_pattern("a{3}");
        let o = ops(&prog);
        assert_eq!(o, vec![Op::Char, Op::Char, Op::Char, Op::Match]);
    }

    #[test]
    fn braced_range_emits_optional_extras() {
        let prog = compile_pattern("a{1,3}");
        // CHAR (mandatory), SPLIT, CHAR, SPLIT, CHAR, MATCH
        let o = ops(&prog);
        assert_eq!(
            o,
            vec![
                Op::Char,
                Op::Split,
                Op::Char,
                Op::Split,
                Op::Char,
                Op::Match
            ]
        );
    }

    #[test]
    fn capturing_group_emits_save_brackets() {
        let prog = compile_pattern("(a)");
        let o = ops(&prog);
        assert_eq!(o, vec![Op::Save, Op::Char, Op::Save, Op::Match]);
        assert_eq!(prog.insts[0].a, 2); // start slot = 2*idx
        assert_eq!(prog.insts[2].a, 3); // end slot = 2*idx+1
    }

    #[test]
    fn non_capturing_group_skips_save() {
        let prog = compile_pattern("(?:a)");
        let o = ops(&prog);
        assert_eq!(o, vec![Op::Char, Op::Match]);
    }

    #[test]
    fn lookahead_compiles_into_sub_program() {
        let prog = compile_pattern("(?=a)b");
        // Main: LOOKAHEAD(sub_idx=0), CHAR b, MATCH
        // Sub:  CHAR a, MATCH
        assert_eq!(prog.insts[0].op, Op::Lookahead as u8);
        assert_eq!(prog.insts[0].a, 0);
        assert_eq!(prog.sub_progs.len(), 1);
        let sub = &prog.sub_progs[0];
        assert_eq!(sub.insts.len(), 2);
        assert_eq!(sub.insts[0].op, Op::Char as u8);
        assert_eq!(sub.insts[0].ch, b'a');
        assert_eq!(sub.insts[1].op, Op::Match as u8);
    }

    #[test]
    fn negative_lookahead_emits_correct_op() {
        let prog = compile_pattern("(?!a)b");
        assert_eq!(prog.insts[0].op, Op::NegLookahead as u8);
    }

    #[test]
    fn backref_emits_op_backref_with_capture_idx() {
        let prog = compile_pattern("(a)\\1");
        // SAVE, CHAR a, SAVE, BACKREF 1, MATCH
        assert_eq!(prog.insts[3].op, Op::Backref as u8);
        assert_eq!(prog.insts[3].a, 1);
    }
}
