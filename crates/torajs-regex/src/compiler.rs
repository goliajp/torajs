//! Thompson NFA construction — port of `runtime_regex.c`
//! L1157-1350 (`compile_node` / `compile_repeat` / `compile_alt`).
//!
//! Walks an AST produced by [`crate::parser`] and emits flat
//! bytecode into a [`Program`]. Backpatching uses the index returned
//! by `Program::emit` + the `next_idx` cursor; jump targets are
//! finalized once the relevant sub-tree is in place.
//!
//! chunk 10d — `flags` threaded through every entry to decide
//! whether the u-flag is active. Under `u`, [`NodeKind::Class`]
//! nodes whose [`CharClass`] is unsafe (negate / non-ASCII bits /
//! `\p{}` fold-ins) are rewritten into a byte-level Alt of Concat
//! by [`crate::utf8_class_expand::expand_unsafe_class`] and the
//! expansion is compiled recursively — the resulting `Op::Class`
//! instructions all reference `CharClass { byte_only: true, .. }`
//! so the Pike VM second-pass also steps a single byte regardless
//! of `u`.

use crate::node::{Node, NodeKind};
use crate::parser::unicode_mode;
use crate::program::{Inst, Op, Program};
use crate::utf8_class_expand::expand_unsafe_class;
use alloc::{boxed::Box, vec::Vec};

/// Compile `node` into `prog`. The emitted bytecode is appended; the
/// caller is responsible for the outer `OP_MATCH` once the root of the
/// whole pattern has been compiled. `flags` carries the active regex
/// flag bits (parsed by [`crate::flags::parse_flags`]) so the u-flag
/// path can rewrite unsafe classes into byte-level alternatives.
pub fn compile(prog: &mut Program, node: &Node, flags: u8) {
    compile_dir(prog, node, flags, false);
}

/// [`compile`] in reverse mode — emits bytecode for the reverse Pike
/// VM ([`crate::vm::match_at_rev`], lookbehind bodies per ES §22.2.2
/// MatchReverse; V8 irregexp `read_backward` shape). Concat children
/// emit in reverse order, capture SAVE slots swap (end slot at group
/// entry, start slot at exit — walking backwards enters a group at
/// its end position), and consuming ops emit unchanged: direction is
/// an execution property of the VM, not of the instruction.
/// Alternation / repeat structure — and thus priority order — is
/// preserved.
pub fn compile_rev(prog: &mut Program, node: &Node, flags: u8) {
    compile_dir(prog, node, flags, true);
}

fn compile_dir(prog: &mut Program, node: &Node, flags: u8, rev: bool) {
    match node.kind {
        NodeKind::Char => {
            emit_with_ims(prog, Inst::char_lit(node.ch), node);
        }
        NodeKind::Any => {
            emit_with_ims(prog, Inst::simple(Op::AnyChar), node);
        }
        NodeKind::Class => {
            let uflag = unicode_mode(flags);
            // Round 3 Path A v2 — K-PROPERTY (pure-property u-flag
            // classes: property tables referenced, no negate, no
            // explicit non-ASCII bits) skip chunk-10d Alt-of-Concat
            // expansion. The DFA
            // build pre-emits a pending K-PROPERTY state whose executor
            // handler decodes one UTF-8 cp per step and consults
            // `class.test_cp(cp)`; the Pike VM second-pass cp-aware
            // branch (`match_at.rs` lines 149-191) already handles the
            // same class. K-NEG / K-EXPLICIT-BIT / K-MIXED still flow
            // through `expand_unsafe_class` (chunk-10d byte-step).
            if uflag && node.cc.is_uflag_property_only() {
                let cidx = prog.intern_class(&node.cc);
                emit_with_ims(prog, Inst::class_ref(cidx), node);
            } else if let Some(mut expansion) = expand_unsafe_class(
                &node.cc,
                uflag,
                node.eff_ims & crate::parser::RE_FLAG_I != 0,
            ) {
                // rev threads through: the expansion is an Alt of
                // per-length Concats of byte-level ops, and reversing
                // those Concats makes the reverse VM consume the
                // multi-byte sequence right-to-left byte by byte.
                // The synthesized leaves inherit the class atom's
                // effective i/m/s scope (regexp-modifiers).
                stamp_eff_ims(&mut expansion, node.eff_ims);
                compile_dir(prog, &expansion, flags, rev);
            } else {
                let cidx = prog.intern_class(&node.cc);
                emit_with_ims(prog, Inst::class_ref(cidx), node);
            }
        }
        NodeKind::AnchorBeg => {
            emit_with_ims(prog, Inst::simple(Op::AnchorB), node);
        }
        NodeKind::AnchorEnd => {
            emit_with_ims(prog, Inst::simple(Op::AnchorE), node);
        }
        NodeKind::WBound => {
            emit_with_ims(prog, Inst::simple(Op::WBound), node);
        }
        NodeKind::NWBound => {
            emit_with_ims(prog, Inst::simple(Op::NWBound), node);
        }
        NodeKind::Concat => {
            if rev {
                for kid in node.kids.iter().rev() {
                    compile_dir(prog, kid, flags, rev);
                }
            } else {
                for kid in &node.kids {
                    compile_dir(prog, kid, flags, rev);
                }
            }
        }
        NodeKind::Alt => compile_alt(prog, node, flags, rev),
        NodeKind::Repeat => compile_repeat(prog, node, flags, rev),
        NodeKind::Group => compile_group(prog, node, flags, rev),
        NodeKind::Backref => {
            emit_with_ims(prog, Inst::backref(node.capture_idx), node);
        }
        NodeKind::Lookahead
        | NodeKind::NegLookahead
        | NodeKind::Lookbehind
        | NodeKind::NegLookbehind => compile_lookaround(prog, node, flags),
    }
}

/// Emit `ins` with the atom's effective i/m/s bits baked into the
/// instruction's `pad` low byte (regexp-modifiers). The VM / DFA read
/// ignoreCase / multiline / dotAll from `Inst.pad` per instruction —
/// bit positions reuse `RE_FLAG_I` / `RE_FLAG_M` / `RE_FLAG_S`, so
/// helpers like [`crate::vm::char_eq`] take the pad byte verbatim.
fn emit_with_ims(prog: &mut Program, mut ins: Inst, node: &Node) {
    ins.pad = node.eff_ims as u16;
    prog.emit(ins);
}

/// Recursively stamp `eff` onto a synthesized sub-tree (chunk-10d
/// `expand_unsafe_class` output) whose nodes were built with the
/// default scope.
fn stamp_eff_ims(node: &mut Node, eff: u8) {
    node.eff_ims = eff;
    if let Some(child) = node.child.as_deref_mut() {
        stamp_eff_ims(child, eff);
    }
    for kid in &mut node.kids {
        stamp_eff_ims(kid, eff);
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
fn compile_alt(prog: &mut Program, node: &Node, flags: u8, rev: bool) {
    let n_alts = node.kids.len();
    if n_alts == 0 {
        return; // defensive — parser doesn't produce empty Alt
    }
    let mut jmps = Vec::with_capacity(n_alts);
    for kid in &node.kids[..n_alts - 1] {
        let sidx = prog.emit(Inst::split(0, 0));
        let branch_start = prog.next_idx();
        compile_dir(prog, kid, flags, rev);
        let jmp_idx = prog.emit(Inst::jmp(0));
        jmps.push(jmp_idx as usize);
        let next = prog.next_idx();
        prog.insts[sidx as usize].a = branch_start;
        prog.insts[sidx as usize].b = next;
    }
    // Last alternative — no trailing JMP; falls through to Lend.
    compile_dir(prog, &node.kids[n_alts - 1], flags, rev);
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
fn compile_repeat(prog: &mut Program, node: &Node, flags: u8, rev: bool) {
    let Some(child) = node.child.as_deref() else {
        return;
    };
    // Unrolled mandatory prefix.
    for _ in 0..node.min {
        compile_dir(prog, child, flags, rev);
    }
    if node.max == -1 {
        compile_kleene_tail(prog, child, node.lazy, flags, rev);
    } else {
        let extra = node.max - node.min;
        compile_bounded_extras(prog, child, extra, node.lazy, flags, rev);
    }
}

/// SPLIT-loop tail for unbounded repeats (`*` / `+` / `{n,}`).
/// Greedy: `SPLIT body, after`; lazy: targets swapped.
fn compile_kleene_tail(prog: &mut Program, child: &Node, lazy: bool, flags: u8, rev: bool) {
    let split_idx = prog.emit(Inst::split(0, 0));
    let body_start = prog.next_idx();
    compile_dir(prog, child, flags, rev);
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
fn compile_bounded_extras(
    prog: &mut Program,
    child: &Node,
    extra: i32,
    lazy: bool,
    flags: u8,
    rev: bool,
) {
    if extra <= 0 {
        return;
    }
    let mut splits = Vec::with_capacity(extra as usize);
    for _ in 0..extra {
        let sidx = prog.emit(Inst::split(0, 0));
        splits.push(sidx as usize);
        let body_start = prog.next_idx();
        compile_dir(prog, child, flags, rev);
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
/// In reverse mode the slots swap: walking backwards enters a group
/// at its END position in the string and exits at its START, so the
/// entry SAVE writes `2*idx+1` and the exit SAVE writes `2*idx` —
/// the recorded `[start, end)` pair stays forward-oriented either way.
fn compile_group(prog: &mut Program, node: &Node, flags: u8, rev: bool) {
    let Some(child) = node.child.as_deref() else {
        return;
    };
    if node.capture_idx > 0 {
        let (enter, exit) = if rev {
            (2 * node.capture_idx + 1, 2 * node.capture_idx)
        } else {
            (2 * node.capture_idx, 2 * node.capture_idx + 1)
        };
        prog.emit(Inst::save(enter));
        compile_dir(prog, child, flags, rev);
        prog.emit(Inst::save(exit));
    } else {
        compile_dir(prog, child, flags, rev);
    }
}

/// `(?=...)` / `(?!...)` / `(?<=...)` / `(?<!...)`. The body compiles
/// into a fresh sub-Program (with its own `OP_MATCH` terminator); the
/// parent emits the appropriate `OP_*_LOOKAHEAD/BEHIND` op pointing at
/// the sub-Program's index. Lookbehind bodies compile in REVERSE mode
/// (ES §22.2.2 MatchReverse) and are probed leftwards by
/// [`crate::vm::match_at_rev`]; the body's own kind decides its
/// direction regardless of any enclosing lookaround, which is exactly
/// the spec's per-assertion direction reset.
fn compile_lookaround(prog: &mut Program, node: &Node, flags: u8) {
    let body_rev = matches!(node.kind, NodeKind::Lookbehind | NodeKind::NegLookbehind);
    let mut sub = Program::new();
    if let Some(child) = node.child.as_deref() {
        compile_dir(&mut sub, child, flags, body_rev);
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
    use alloc::vec;

    fn compile_pattern(pat: &str) -> Program {
        let mut p = Parser::new(pat.as_bytes(), 0);
        let root = p.parse().expect("parse failed");
        let mut prog = Program::new();
        compile(&mut prog, &root, 0);
        prog.emit(Inst::match_accept());
        prog
    }

    fn compile_pattern_uflag(pat: &str) -> Program {
        let flags = crate::parser::RE_FLAG_U;
        let mut p = Parser::new(pat.as_bytes(), flags);
        let root = p.parse().expect("parse failed");
        let mut prog = Program::new();
        compile(&mut prog, &root, flags);
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
    fn lookbehind_body_compiles_reversed() {
        // `(?<=ab)c` — the lookbehind body sub-program is reverse-
        // compiled: CHAR b, CHAR a, MATCH.
        let prog = compile_pattern("(?<=ab)c");
        assert_eq!(prog.insts[0].op, Op::Lookbehind as u8);
        let sub = &prog.sub_progs[0];
        assert_eq!(sub.insts[0].op, Op::Char as u8);
        assert_eq!(sub.insts[0].ch, b'b');
        assert_eq!(sub.insts[1].op, Op::Char as u8);
        assert_eq!(sub.insts[1].ch, b'a');
        assert_eq!(sub.insts[2].op, Op::Match as u8);
    }

    #[test]
    fn lookahead_nested_in_lookbehind_body_stays_forward() {
        // `(?<=a(?=bc))b` — the inner lookahead's own body compiles
        // FORWARD (per-assertion direction reset) even though the
        // enclosing lookbehind body is reversed.
        let prog = compile_pattern("(?<=a(?=bc))b");
        let behind = &prog.sub_progs[0];
        // Reversed body: [Lookahead, Char a, Match].
        assert_eq!(behind.insts[0].op, Op::Lookahead as u8);
        assert_eq!(behind.insts[1].ch, b'a');
        let ahead = &behind.sub_progs[behind.insts[0].a as usize];
        assert_eq!(ahead.insts[0].ch, b'b');
        assert_eq!(ahead.insts[1].ch, b'c');
    }

    #[test]
    fn backref_emits_op_backref_with_capture_idx() {
        let prog = compile_pattern("(a)\\1");
        // SAVE, CHAR a, SAVE, BACKREF 1, MATCH
        assert_eq!(prog.insts[3].op, Op::Backref as u8);
        assert_eq!(prog.insts[3].a, 1);
    }

    #[test]
    fn uflag_safe_class_keeps_single_op_class() {
        // `\d` under u flag is u-safe (ASCII-only, no negate, no
        // property tables) — chunk 10d expansion is a no-op, so we still
        // emit a single OP_CLASS pointing at an interned class.
        let prog = compile_pattern_uflag("\\d");
        assert_eq!(prog.insts[0].op, Op::Class as u8);
        assert_eq!(prog.classes.len(), 1);
        assert!(!prog.classes[0].byte_only);
    }

    #[test]
    fn uflag_unsafe_negate_class_expands_into_byte_only_classes() {
        // `[^a]u` triggers the chunk 10d AST rewrite. The leaf
        // classes are `byte_only`, and the expansion produces an Alt
        // over length groups so the bytecode starts with a SPLIT.
        let prog = compile_pattern_uflag("[^a]");
        assert!(prog.classes.iter().any(|c| c.byte_only));
        let class_count = prog
            .insts
            .iter()
            .filter(|i| i.op == Op::Class as u8)
            .count();
        assert!(class_count >= 2, "expected >=2 OP_CLASS, got {class_count}");
    }

    fn compile_pattern_rev(pat: &str) -> Program {
        let mut p = Parser::new(pat.as_bytes(), 0);
        let root = p.parse().expect("parse failed");
        let mut prog = Program::new();
        compile_rev(&mut prog, &root, 0);
        prog.emit(Inst::match_accept());
        prog
    }

    #[test]
    fn rev_concat_emits_reversed_sequence() {
        let prog = compile_pattern_rev("abc");
        assert_eq!(ops(&prog), vec![Op::Char, Op::Char, Op::Char, Op::Match]);
        assert_eq!(prog.insts[0].ch, b'c');
        assert_eq!(prog.insts[1].ch, b'b');
        assert_eq!(prog.insts[2].ch, b'a');
    }

    #[test]
    fn rev_group_swaps_save_slots() {
        let prog = compile_pattern_rev("(a)");
        let o = ops(&prog);
        assert_eq!(o, vec![Op::Save, Op::Char, Op::Save, Op::Match]);
        assert_eq!(prog.insts[0].a, 3); // entry = end slot (2*idx+1)
        assert_eq!(prog.insts[2].a, 2); // exit = start slot (2*idx)
    }

    #[test]
    fn rev_alt_keeps_branch_order_reverses_contents() {
        // `ab|c` reversed: SPLIT, [b, a], JMP, [c], MATCH — branch
        // priority order unchanged, each branch's Concat reversed.
        let prog = compile_pattern_rev("ab|c");
        let o = ops(&prog);
        assert_eq!(
            o,
            vec![Op::Split, Op::Char, Op::Char, Op::Jmp, Op::Char, Op::Match]
        );
        assert_eq!(prog.insts[1].ch, b'b');
        assert_eq!(prog.insts[2].ch, b'a');
        assert_eq!(prog.insts[4].ch, b'c');
    }

    #[test]
    fn rev_repeat_structure_preserved_child_reversed() {
        // `(?:ab)*` reversed: SPLIT, [b, a], JMP → SPLIT, MATCH.
        let prog = compile_pattern_rev("(?:ab)*");
        let o = ops(&prog);
        assert_eq!(o, vec![Op::Split, Op::Char, Op::Char, Op::Jmp, Op::Match]);
        assert_eq!(prog.insts[0].a, 1); // greedy: body first
        assert_eq!(prog.insts[1].ch, b'b');
        assert_eq!(prog.insts[2].ch, b'a');
    }

    #[test]
    fn uflag_property_letter_emits_single_op_class() {
        // Round 3 Path A v2 — `\p{L}/u` is K-PROPERTY (property tables
        // referenced, no negate, no explicit non-ASCII bits). Compiler
        // emits a single `Op::Class` referencing the cp-range form; the
        // DFA build pre-emits a pending K-PROPERTY state whose executor
        // handler decodes one UTF-8 cp per step under u-flag. Was a
        // SPLIT cascade from `utf8_class_expand` Alt-of-Concat
        // (chunk-10d).
        let prog = compile_pattern_uflag("\\p{L}");
        assert_eq!(prog.insts[0].op, Op::Class as u8);
        assert_eq!(prog.classes.len(), 1);
        // K-PROPERTY class is NOT marked byte_only — the executor
        // consults `test_cp(cp)`, not `test(byte)`.
        assert!(!prog.classes[0].byte_only);
        assert!(!prog.classes[0].u_prop_tables.is_empty());
        assert!(prog.classes[0].is_uflag_property_only());
    }
}
