//! Release splitting for `any` values — the other half of the ARC
//! code motion in [`super::inc_sink`]: a `rc_dec_any` that opens a
//! join moves up onto the edges that reach it, and goes from every
//! edge that proved the value a number.
//!
//! `d[k] = (d[k] ?? 0) + 1` reads an owned box, tests it, adds on an
//! int arm, a double arm and a kernel arm, and releases the read box
//! where the three arms meet. Only the kernel arm can hold a cell
//! there; on the two number arms the release is a no-op, yet its
//! inline cell test and taken branch sit in the hot path of each
//! iteration.
//!
//! The release is the first instruction of its block, so the end of
//! each predecessor whose only successor is that block is the same
//! point of execution: moving it there changes nothing. On an edge
//! that proves the value a number (`rc_fold`'s facts) it is dropped,
//! since releasing a number is a no-op. A release is split only when
//! some edge proves the value and at most one edge needs a copy, so
//! the code never grows; an edge that needs a copy out of a block
//! with other successors keeps the release where it is.
//!
//! Runs right after the retain sinking, on the releases it left at
//! the head of a block: a retain sunk there keeps the releases behind
//! it in place. Both read the facts of [`super::motion_facts`]. `TORAJS_ANYNUM_DEC_SPLIT_OFF=1` skips,
//! `TORAJS_ANYNUM_DEC_SPLIT_STATS=1` dumps counters.

use crate::id_hash::HashSet;

use torajs_core::ssa::{BlockId, Function, InstKind, Module, Operand, ValueId};

use crate::anynum::motion_facts::MotionFacts;
use crate::anynum::number_facts::NumberFacts;
use crate::anynum::preds::Preds;
use crate::float_demote::cfg::successors;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DecSplitStats {
    /// Releases moved off the head of a join.
    pub releases_split: u32,
    /// Copies placed for them on the edges that did not prove a number.
    pub releases_placed: u32,
}

pub fn split_join_releases(module: &mut Module, motion: &MotionFacts) -> DecSplitStats {
    let mut stats = DecSplitStats::default();
    for (f, func) in module.funcs.iter_mut().enumerate() {
        let dense = func
            .blocks
            .iter()
            .enumerate()
            .all(|(i, b)| b.id.0 as usize == i);
        if func.is_declaration() || !dense {
            continue;
        }
        let cands = candidates(func);
        if cands.is_empty() {
            continue;
        }
        // the candidates are among the releases the facts always cover
        let Some(facts) = motion.facts(f, func, Vec::new()) else {
            continue;
        };
        let preds = motion.preds(f, func);
        let (split, placed) = split(func, facts, preds, &cands);
        stats.releases_split += split;
        stats.releases_placed += placed;
    }
    stats
}

/// The releases that open a block other than the entry: (block,
/// value) for each `rc_dec_any` of the leading run.
fn candidates(func: &Function) -> Vec<(usize, ValueId)> {
    let mut out = Vec::new();
    for (b, block) in func.blocks.iter().enumerate().skip(1) {
        for inst in &block.insts {
            let InstKind::RcDecAny(Operand::Value(v)) = inst.kind else {
                break;
            };
            out.push((b, v));
        }
    }
    out
}

/// Split the releases [`candidates`] found, by `facts` computed for
/// their values; (releases split, copies placed).
fn split(
    func: &mut Function,
    facts: &NumberFacts,
    preds: &Preds,
    cands: &[(usize, ValueId)],
) -> (u32, u32) {
    let mut moves: Vec<(usize, ValueId, Option<usize>)> = Vec::new();
    for &(b, v) in cands {
        if let Some(to) = plan(func, facts, preds, b, v) {
            moves.push((b, v, to));
        }
    }
    let (mut split, mut placed) = (0, 0);
    for (b, v, to) in moves {
        let block = &mut func.blocks[b];
        let Some(at) = block
            .insts
            .iter()
            .take_while(|i| matches!(i.kind, InstKind::RcDecAny(_)))
            .position(|i| matches!(i.kind, InstKind::RcDecAny(Operand::Value(x)) if x == v))
        else {
            continue;
        };
        let inst = block.insts.remove(at);
        split += 1;
        if let Some(p) = to {
            func.blocks[p].insts.push(inst);
            placed += 1;
        }
    }
    (split, placed)
}

/// The one predecessor that keeps the release of `v` opening block
/// `b` (`Some(None)` when every edge proves `v`), or `None` when the
/// release stays.
fn plan(
    func: &Function,
    facts: &NumberFacts,
    preds: &Preds,
    b: usize,
    v: ValueId,
) -> Option<Option<usize>> {
    let mut seen: HashSet<usize> = HashSet::default();
    let mut keep: Option<usize> = None;
    let mut proven = false;
    for &p in preds.of_block(b) {
        if !seen.insert(p) {
            continue;
        }
        if facts.proven_on_edge(p, BlockId(b as u32), v) {
            proven = true;
            continue;
        }
        let only_here = successors(&func.blocks[p].term).all(|s| s.0 as usize == b);
        if keep.is_some() || p == b || !only_here {
            return None;
        }
        keep = Some(p);
    }
    proven.then_some(keep)
}

#[cfg(test)]
mod tests {
    use super::{DecSplitStats, split_join_releases};
    use crate::anynum::inc_sink::sink_number_retains;
    use crate::anynum::motion_facts::MotionFacts;
    use crate::pass_gates::gated_pass;
    use torajs_core::ssa::{
        Block, BlockId, FuncId, Function, Inst, InstKind, Module, Operand, Terminator, Type,
        ValueId, ValueInfo,
    };

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn inst(result: Option<u32>, kind: InstKind) -> Inst {
        Inst {
            result: result.map(ValueId),
            kind,
            origin: None,
        }
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn cond(c: u32, t: u32, e: u32) -> Terminator {
        Terminator::CondBr {
            cond: v(c),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        }
    }

    fn module(blocks: Vec<Block>) -> Module {
        let mut m = Module::default();
        m.funcs.push(Function {
            name: "f".into(),
            params: vec![ValueId(0), ValueId(2)],
            ret: Type::Void,
            blocks,
            values: [Type::Any, Type::Bool, Type::Bool]
                .iter()
                .map(|t| ValueInfo { ty: *t, name: None })
                .collect(),
            current_origin: None,
        });
        m
    }

    fn decs(m: &Module, b: usize) -> usize {
        m.funcs[0].blocks[b]
            .insts
            .iter()
            .filter(|i| matches!(i.kind, InstKind::RcDecAny(_)))
            .count()
    }

    fn split(m: &mut Module) -> DecSplitStats {
        let motion = MotionFacts::new(m);
        split_join_releases(m, &motion)
    }

    fn call() -> Inst {
        inst(None, InstKind::Call(FuncId(0), vec![v(0)]))
    }

    /// The int arm proves `%0` a number, the call arm does not: the
    /// release opening the join moves to the end of the call arm and
    /// the int arm carries none.
    #[test]
    fn a_join_release_moves_onto_the_one_arm_that_proves_nothing() {
        let mut m = module(vec![
            block(
                0,
                vec![inst(Some(1), InstKind::AnyIsInt32(v(0)))],
                cond(1, 1, 2),
            ),
            block(1, vec![], Terminator::Br(BlockId(3))),
            block(2, vec![call()], Terminator::Br(BlockId(3))),
            block(
                3,
                vec![inst(None, InstKind::RcDecAny(v(0))), call()],
                Terminator::Ret(None),
            ),
        ]);
        let st = split(&mut m);
        assert_eq!((st.releases_split, st.releases_placed), (1, 1));
        assert_eq!((decs(&m, 1), decs(&m, 2), decs(&m, 3)), (0, 1, 0));
        // behind the arm's own call, where the join used to start
        assert!(matches!(
            m.funcs[0].blocks[2].insts[1].kind,
            InstKind::RcDecAny(_)
        ));
    }

    /// Two arms prove nothing: splitting would copy the release, so it
    /// stays.
    #[test]
    fn a_release_two_edges_would_need_stays() {
        let mut m = module(vec![
            block(
                0,
                vec![inst(Some(1), InstKind::AnyIsInt32(v(0)))],
                cond(1, 1, 4),
            ),
            block(1, vec![], Terminator::Br(BlockId(3))),
            block(2, vec![call()], Terminator::Br(BlockId(3))),
            block(
                3,
                vec![inst(None, InstKind::RcDecAny(v(0)))],
                Terminator::Ret(None),
            ),
            block(4, vec![], cond(2, 2, 3)),
        ]);
        let st = split(&mut m);
        assert_eq!(st.releases_split, 0);
        assert_eq!(decs(&m, 3), 1);
    }

    /// The arm that proves nothing branches elsewhere too: the release
    /// has no place on its edge alone, so it stays.
    #[test]
    fn a_release_whose_edge_leaves_a_branching_block_stays() {
        let mut m = module(vec![
            block(
                0,
                vec![inst(Some(1), InstKind::AnyIsInt32(v(0)))],
                cond(1, 1, 2),
            ),
            block(1, vec![], Terminator::Br(BlockId(3))),
            block(2, vec![call()], cond(2, 3, 4)),
            block(
                3,
                vec![inst(None, InstKind::RcDecAny(v(0)))],
                Terminator::Ret(None),
            ),
            block(4, vec![], Terminator::Ret(None)),
        ]);
        let st = split(&mut m);
        assert_eq!(st.releases_split, 0);
        assert_eq!(decs(&m, 3), 1);
    }

    /// A retain of the same value sinks to the head of the join (the
    /// release stops its walk): the release behind it stays behind it.
    #[test]
    fn a_release_behind_a_sunk_retain_stays_behind_it() {
        let mut m = module(vec![
            block(
                0,
                vec![
                    inst(None, InstKind::RcIncAny(v(0))),
                    inst(Some(1), InstKind::AnyIsNumber(v(0))),
                ],
                cond(1, 1, 2),
            ),
            block(1, vec![], Terminator::Br(BlockId(3))),
            block(2, vec![], Terminator::Br(BlockId(3))),
            block(
                3,
                vec![inst(None, InstKind::RcDecAny(v(0))), call()],
                Terminator::Ret(None),
            ),
        ]);
        // one set of facts for both, as the pipeline runs them
        let motion = MotionFacts::new(&m);
        assert_eq!(sink_number_retains(&mut m, &motion).retains_sunk, 1);
        assert_eq!(split_join_releases(&mut m, &motion).releases_split, 0);
        let b3 = &m.funcs[0].blocks[3].insts;
        assert!(matches!(b3[0].kind, InstKind::RcIncAny(_)));
        assert!(matches!(b3[1].kind, InstKind::RcDecAny(_)));
        assert_eq!(decs(&m, 2), 0);
    }

    /// `TORAJS_ANYNUM_DEC_SPLIT_OFF=1` leaves the module as it was on
    /// the shape the pass would split.
    #[test]
    fn the_off_gate_leaves_the_ir_alone() {
        let join = || {
            module(vec![
                block(
                    0,
                    vec![inst(Some(1), InstKind::AnyIsInt32(v(0)))],
                    cond(1, 1, 2),
                ),
                block(1, vec![], Terminator::Br(BlockId(3))),
                block(2, vec![call()], Terminator::Br(BlockId(3))),
                block(
                    3,
                    vec![inst(None, InstKind::RcDecAny(v(0))), call()],
                    Terminator::Ret(None),
                ),
            ])
        };
        let mut on = join();
        assert!(gated_pass("ANYNUM_DEC_SPLIT", &mut on, split).is_some());
        assert_eq!(decs(&on, 3), 0);
        let mut off = join();
        let before = format!("{off:?}");
        // SAFETY: nextest runs each test in its own process, so no
        // other thread reads the variable while it is set
        unsafe {
            std::env::set_var("TORAJS_ANYNUM_DEC_SPLIT_OFF", "1");
        }
        let ran = gated_pass("ANYNUM_DEC_SPLIT", &mut off, split);
        unsafe {
            std::env::remove_var("TORAJS_ANYNUM_DEC_SPLIT_OFF");
        }
        assert!(ran.is_none());
        assert_eq!(format!("{off:?}"), before);
    }
}
