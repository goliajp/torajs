//! Retain sinking for `any` values — the ARC code motion that moves a
//! `rc_inc_any` down to the paths that can observe the count.
//!
//! `const x = arr[i]; total = total + x` retains the element as soon
//! as it is read, then tests it: the number arms of the `+` add the
//! bits and never touch the count, and only the kernel arm (a cell
//! addend) runs a call that could drop the slot's own share. The
//! retain therefore does nothing on every path the loop stays on,
//! yet its inline cell test sits in the hot path of each iteration —
//! two instructions and a taken branch on a loop that is bound by
//! taken branches.
//!
//! The pass walks a retain forward: past retain-transparent
//! instructions (pure ops, loads other than a count read, copies;
//! nothing that can lower a count or write a slot), following a `copy any` of the retained
//! value onto its result, and down a branch onto every edge that
//! does not prove the value a number (`rc_fold`'s facts: the true
//! edge of `any_is_number` / `any_is_int32`, a box). It stops before
//! the first instruction that is not transparent or that redefines
//! the value, and before `ret`, and the retain is placed there. A
//! join is entered only when every predecessor edge either carries
//! the sunk retain or proves the value a number — anywhere else the
//! original path structure would change — and never twice (a cycle
//! would retain once per trip) nor into a deeper loop. A retain the
//! walk cannot place that way stays where it is.
//!
//! Moving a retain later is sound while nothing between the two
//! positions can release the object: the holder that kept it alive
//! at the original position (the array slot, the binding's own
//! share) still does at the new one, since only transparent
//! instructions ran in between. A copy where the value is proven a
//! number is simply dropped: retaining a number is a no-op.
//!
//! Runs after the loop versioning and the borrow / pair passes, on
//! the retains those left, reading the tests in their expanded bit
//! form. `TORAJS_ANYNUM_INC_SINK_OFF=1` skips,
//! `TORAJS_ANYNUM_INC_SINK_STATS=1` dumps counters.

use std::cell::OnceCell;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;
use torajs_core::ssa::{BlockId, Function, Inst, InstKind, Module, Operand, Terminator, ValueId};

use crate::anynum::number_facts::NumberFacts;
use crate::anynum::preds::Preds;
use crate::anynum::retain_walk::{Walk, exit_at, walk};
use crate::float_demote::cfg::successors;

/// Copies of one retain past which the walk gives up: beyond this
/// the code grows faster than the hot path shrinks.
const MAX_PLACEMENTS: usize = 4;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IncSinkStats {
    /// Retains moved off their original site.
    pub retains_sunk: u32,
    /// Copies placed for them (fewer than the retains when a copy was
    /// dropped as a proven number).
    pub retains_placed: u32,
}

pub fn sink_number_retains(module: &mut Module) -> IncSinkStats {
    let mut stats = IncSinkStats::default();
    for func in module.funcs.iter_mut() {
        let dense = func
            .blocks
            .iter()
            .enumerate()
            .all(|(i, b)| b.id.0 as usize == i);
        if func.is_declaration() || !dense {
            continue;
        }
        let (sunk, placed) = sink_in_function(func);
        stats.retains_sunk += sunk;
        stats.retains_placed += placed;
    }
    stats
}

/// (block index, instruction index).
type Site = (usize, usize);

fn sink_in_function(func: &mut Function) -> (u32, u32) {
    let mut sites: Vec<(Site, ValueId)> = Vec::new();
    for (b, block) in func.blocks.iter().enumerate() {
        for (i, inst) in block.insts.iter().enumerate() {
            if let InstKind::RcIncAny(Operand::Value(v)) = &inst.kind {
                sites.push(((b, i), *v));
            }
        }
    }
    if sites.is_empty() {
        return (0, 0);
    }
    // without a test that proves a value a number, every path keeps the
    // retain and the walk could only move it about
    let seeds: Vec<ValueId> = sites.iter().map(|&(_, v)| v).collect();
    let Some(facts) = NumberFacts::compute_if_any_edge_proof(func, &seeds) else {
        return (0, 0);
    };
    let shape = Shape {
        func,
        facts,
        cfg: OnceCell::new(),
    };
    let mut remove: HashSet<Site> = HashSet::default();
    let mut place: Vec<(Site, Inst)> = Vec::new();
    for (site, v) in sites {
        let Some(placements) = plan(func, &shape, site, v) else {
            continue;
        };
        remove.insert(site);
        let like = &func.blocks[site.0].insts[site.1];
        for (at, val) in placements {
            place.push((
                at,
                Inst {
                    result: None,
                    kind: InstKind::RcIncAny(Operand::Value(val)),
                    origin: like.origin,
                },
            ));
        }
    }
    let (sunk, placed) = (remove.len() as u32, place.len() as u32);
    if sunk == 0 {
        return (0, 0);
    }
    let mut before: HashMap<Site, Vec<Inst>> = HashMap::default();
    let mut touched: Vec<usize> = remove.iter().map(|at| at.0).collect();
    for (at, inst) in place {
        touched.push(at.0);
        before.entry(at).or_default().push(inst);
    }
    touched.sort_unstable();
    touched.dedup();
    for b in touched {
        let block = &mut func.blocks[b];
        let old = std::mem::take(&mut block.insts);
        let n = old.len();
        let mut out = Vec::with_capacity(n + 1);
        for (i, inst) in old.into_iter().enumerate() {
            if let Some(extra) = before.remove(&(b, i)) {
                out.extend(extra);
            }
            if !remove.contains(&(b, i)) {
                out.push(inst);
            }
        }
        if let Some(extra) = before.remove(&(b, n)) {
            out.extend(extra);
        }
        block.insts = out;
    }
    (sunk, placed)
}

struct Shape<'f> {
    func: &'f Function,
    facts: NumberFacts,
    /// The loops and the predecessors, built when a walk first
    /// reaches a block it might enter: most retains stop before.
    cfg: OnceCell<(LoopAnalysis, Preds)>,
}

impl Shape<'_> {
    fn cfg(&self) -> &(LoopAnalysis, Preds) {
        self.cfg.get_or_init(|| {
            let dom = DominatorTree::compute(self.func);
            (LoopAnalysis::compute(self.func, &dom), Preds::of(self.func))
        })
    }

    /// Is every loop around `b` also around `home`? A retain placed
    /// in a loop the site is outside of would run once per trip.
    fn within_loops_of(&self, b: usize, home: usize) -> bool {
        let (b, home) = (BlockId(b as u32), BlockId(home as u32));
        self.cfg()
            .0
            .loops()
            .iter()
            .all(|l| !l.contains(&b) || l.contains(&home))
    }

    fn preds(&self, b: usize) -> &[usize] {
        self.cfg().1.of_block(b)
    }
}

/// The placements of the retain at `site` once sunk, or `None` when
/// it stays. An empty answer means every path proved the value a
/// number.
fn plan(func: &Function, shape: &Shape, site: Site, v: ValueId) -> Option<Vec<(Site, ValueId)>> {
    let mut out: Vec<(Site, ValueId)> = Vec::new();
    let mut visited: HashSet<usize> = HashSet::default();
    visited.insert(site.0);
    // blocks whose exit holds a placed copy: their out-edges carry none
    let mut placed_end: HashSet<usize> = HashSet::default();
    let mut work: Vec<(usize, usize, ValueId)> = vec![(site.0, site.1 + 1, v)];
    // edges the retain reached, by target
    let mut pending: HashMap<usize, Vec<(usize, ValueId)>> = HashMap::default();
    loop {
        while let Some((b, start, val)) = work.pop() {
            match walk(func, b, start, val) {
                Walk::Place(k, val) => out.push(((b, k), val)),
                Walk::Exit(val) => {
                    let block = &func.blocks[b];
                    if matches!(block.term, Terminator::Ret(_) | Terminator::Unreachable) {
                        out.push(((b, block.insts.len()), val));
                        continue;
                    }
                    for s in successors(&block.term) {
                        if shape.facts.proven_on_edge(b, s, val) {
                            continue;
                        }
                        pending.entry(s.0 as usize).or_default().push((b, val));
                    }
                }
            }
            if out.len() > MAX_PLACEMENTS {
                return None;
            }
        }
        if pending.is_empty() {
            // a move within the site's own block buys nothing
            return (!out.iter().all(|(at, _)| at.0 == site.0)).then_some(out);
        }
        // A target is entered when every predecessor edge carries the
        // retain or proves the value a number; the walk never enters
        // a block twice or a loop the site is outside of (a loop
        // header's back edge is a predecessor nothing carries, so a
        // loop is never entered through its header either). Anything
        // else places the
        // retain at the exit of each block that reached it, which
        // takes those blocks' other edges out of every target.
        loop {
            let targets: Vec<usize> = pending.keys().copied().collect();
            let mut settled = true;
            for s in targets {
                let edges = &pending[&s];
                let val = edges[0].1;
                let ok = edges
                    .iter()
                    .all(|(e, x)| *x == val && !placed_end.contains(e))
                    && !visited.contains(&s)
                    && shape.within_loops_of(s, site.0)
                    && shape.preds(s).iter().all(|&p| {
                        edges.iter().any(|(e, _)| *e == p)
                            || shape.facts.proven_on_edge(p, BlockId(s as u32), val)
                    });
                if ok {
                    continue;
                }
                for (p, val) in pending.remove(&s).expect("a listed target") {
                    if placed_end.insert(p) {
                        let start = if p == site.0 { site.1 + 1 } else { 0 };
                        out.push(((p, exit_at(func, p, start)), val));
                    }
                }
                settled = false;
            }
            if settled {
                break;
            }
        }
        if out.len() > MAX_PLACEMENTS {
            return None;
        }
        for (s, edges) in std::mem::take(&mut pending) {
            visited.insert(s);
            work.push((s, 0, edges[0].1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, FuncId, IPred, Type, ValueInfo};

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

    fn module(params: Vec<u32>, types: &[Type], blocks: Vec<Block>) -> Module {
        let mut m = Module::default();
        m.funcs.push(Function {
            name: "f".into(),
            params: params.into_iter().map(ValueId).collect(),
            ret: Type::Void,
            blocks,
            values: types
                .iter()
                .map(|t| ValueInfo { ty: *t, name: None })
                .collect(),
            current_origin: None,
        });
        m
    }

    fn incs(block: &Block) -> Vec<ValueId> {
        block
            .insts
            .iter()
            .filter_map(|i| match i.kind {
                InstKind::RcIncAny(Operand::Value(x)) => Some(x),
                _ => None,
            })
            .collect()
    }

    /// `%0` retained, copied to `%2`, tested: the number arm (bb1)
    /// gets nothing, the call arm (bb2) gets the retain of the copy
    /// ahead of its call.
    #[test]
    fn a_retain_sinks_onto_the_cell_arm_as_the_copy() {
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![
                    inst(None, InstKind::RcIncAny(v(0))),
                    inst(Some(2), InstKind::Copy(Type::Any, v(0))),
                    inst(Some(1), InstKind::AnyIsNumber(v(2))),
                ],
                term: Terminator::CondBr {
                    cond: v(1),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![inst(Some(3), InstKind::AnyToInt32(v(2)))],
                term: Terminator::Br(BlockId(3)),
            },
            Block {
                id: BlockId(2),
                insts: vec![
                    inst(Some(4), InstKind::PtrToInt(v(2))),
                    inst(None, InstKind::Call(FuncId(0), vec![v(2)])),
                ],
                term: Terminator::Br(BlockId(3)),
            },
            Block {
                id: BlockId(3),
                insts: vec![inst(None, InstKind::RcDecAny(v(2)))],
                term: Terminator::Ret(None),
            },
        ];
        let mut m = module(
            vec![0],
            &[Type::Any, Type::Bool, Type::Any, Type::I64, Type::I64],
            blocks,
        );
        let st = sink_number_retains(&mut m);
        assert_eq!(
            st,
            IncSinkStats {
                retains_sunk: 1,
                retains_placed: 1
            }
        );
        let f = &m.funcs[0];
        assert!(incs(&f.blocks[0]).is_empty());
        assert!(incs(&f.blocks[1]).is_empty());
        assert_eq!(incs(&f.blocks[2]), vec![ValueId(2)]);
        // ahead of the call, behind the pure cast
        assert!(matches!(f.blocks[2].insts[1].kind, InstKind::RcIncAny(_)));
        assert_eq!(incs(&f.blocks[3]), Vec::<ValueId>::new());
    }

    /// The join after the retain has a predecessor the retain never
    /// reaches and that proves nothing: the retain stays.
    #[test]
    fn a_join_with_an_unrelated_predecessor_keeps_the_retain() {
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::CondBr {
                    cond: v(1),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![inst(None, InstKind::RcIncAny(v(0)))],
                term: Terminator::Br(BlockId(3)),
            },
            Block {
                id: BlockId(2),
                insts: vec![],
                term: Terminator::Br(BlockId(3)),
            },
            Block {
                id: BlockId(3),
                insts: vec![inst(None, InstKind::Call(FuncId(0), vec![v(0)]))],
                term: Terminator::Ret(None),
            },
        ];
        let mut m = module(vec![0, 1], &[Type::Any, Type::Bool], blocks);
        let st = sink_number_retains(&mut m);
        assert_eq!(st, IncSinkStats::default());
        assert_eq!(incs(&m.funcs[0].blocks[1]), vec![ValueId(0)]);
    }

    /// The cell arm boxes a lane and joins code the retain never
    /// reaches: the retain lands at the arm's exit, the number arm
    /// gets nothing.
    #[test]
    fn a_retain_lands_at_the_exit_before_a_join_it_cannot_enter() {
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::CondBr {
                    cond: v(5),
                    then_blk: BlockId(1),
                    else_blk: BlockId(4),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![
                    inst(None, InstKind::RcIncAny(v(0))),
                    inst(Some(2), InstKind::Copy(Type::Any, v(0))),
                    inst(Some(1), InstKind::AnyIsNumber(v(2))),
                ],
                term: Terminator::CondBr {
                    cond: v(1),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            },
            Block {
                id: BlockId(2),
                insts: vec![inst(Some(3), InstKind::AnyToInt32(v(2)))],
                term: Terminator::Br(BlockId(5)),
            },
            Block {
                id: BlockId(3),
                insts: vec![inst(Some(4), InstKind::BoxInt32(Operand::ConstI64(1)))],
                term: Terminator::Br(BlockId(5)),
            },
            Block {
                id: BlockId(4),
                insts: vec![],
                term: Terminator::Br(BlockId(5)),
            },
            Block {
                id: BlockId(5),
                insts: vec![inst(None, InstKind::Call(FuncId(0), vec![v(2)]))],
                term: Terminator::Ret(None),
            },
        ];
        let mut m = module(
            vec![0, 5],
            &[
                Type::Any,
                Type::Bool,
                Type::Any,
                Type::I64,
                Type::Any,
                Type::Bool,
            ],
            blocks,
        );
        let st = sink_number_retains(&mut m);
        assert_eq!(
            st,
            IncSinkStats {
                retains_sunk: 1,
                retains_placed: 1
            }
        );
        let f = &m.funcs[0];
        assert!(incs(&f.blocks[1]).is_empty());
        assert!(incs(&f.blocks[2]).is_empty());
        assert!(matches!(f.blocks[3].insts[1].kind, InstKind::RcIncAny(_)));
        assert!(incs(&f.blocks[5]).is_empty());
    }

    /// A retain whose every path proves the value a number goes
    /// without a copy anywhere.
    #[test]
    fn a_retain_at_an_exit_goes_ahead_of_the_compare_the_branch_reads() {
        // as above, but `bb3` ends in its own test: the retain lands
        // before the compare, which stays next to the branch
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::CondBr {
                    cond: v(5),
                    then_blk: BlockId(1),
                    else_blk: BlockId(4),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![
                    inst(None, InstKind::RcIncAny(v(0))),
                    inst(Some(2), InstKind::Copy(Type::Any, v(0))),
                    inst(Some(1), InstKind::AnyIsNumber(v(2))),
                ],
                term: Terminator::CondBr {
                    cond: v(1),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            },
            Block {
                id: BlockId(2),
                insts: vec![inst(Some(3), InstKind::AnyToInt32(v(2)))],
                term: Terminator::Br(BlockId(5)),
            },
            Block {
                id: BlockId(3),
                insts: vec![
                    inst(Some(4), InstKind::BoxInt32(Operand::ConstI64(1))),
                    inst(
                        Some(6),
                        InstKind::ICmp(IPred::Ne, v(7), Operand::ConstI64(0)),
                    ),
                ],
                term: Terminator::CondBr {
                    cond: v(6),
                    then_blk: BlockId(5),
                    else_blk: BlockId(4),
                },
            },
            Block {
                id: BlockId(4),
                insts: vec![],
                term: Terminator::Br(BlockId(5)),
            },
            Block {
                id: BlockId(5),
                insts: vec![inst(None, InstKind::Call(FuncId(0), vec![v(2)]))],
                term: Terminator::Ret(None),
            },
        ];
        let mut m = module(
            vec![0, 5, 7],
            &[
                Type::Any,
                Type::Bool,
                Type::Any,
                Type::I64,
                Type::Any,
                Type::Bool,
                Type::Bool,
                Type::I64,
            ],
            blocks,
        );
        assert_eq!(sink_number_retains(&mut m).retains_placed, 1);
        let b3 = &m.funcs[0].blocks[3].insts;
        assert!(matches!(b3[1].kind, InstKind::RcIncAny(_)));
        assert!(matches!(b3[2].kind, InstKind::ICmp(..)));
    }

    #[test]
    fn a_retain_every_path_proves_a_number_goes() {
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![
                    inst(None, InstKind::RcIncAny(v(0))),
                    inst(Some(1), InstKind::AnyIsInt32(v(0))),
                ],
                term: Terminator::CondBr {
                    cond: v(1),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![inst(None, InstKind::Call(FuncId(0), vec![v(0)]))],
                term: Terminator::Ret(None),
            },
            Block {
                id: BlockId(2),
                insts: vec![inst(Some(2), InstKind::AnyIsNumber(v(0)))],
                term: Terminator::CondBr {
                    cond: v(2),
                    then_blk: BlockId(3),
                    else_blk: BlockId(4),
                },
            },
            Block {
                id: BlockId(3),
                insts: vec![inst(None, InstKind::Call(FuncId(0), vec![v(0)]))],
                term: Terminator::Ret(None),
            },
            Block {
                id: BlockId(4),
                insts: vec![inst(None, InstKind::Call(FuncId(0), vec![v(0)]))],
                term: Terminator::Ret(None),
            },
        ];
        let mut m = module(vec![0], &[Type::Any, Type::Bool, Type::Bool], blocks);
        let st = sink_number_retains(&mut m);
        assert_eq!(
            st,
            IncSinkStats {
                retains_sunk: 1,
                retains_placed: 1
            }
        );
        let f = &m.funcs[0];
        assert!(incs(&f.blocks[0]).is_empty());
        assert!(incs(&f.blocks[1]).is_empty());
        assert!(incs(&f.blocks[3]).is_empty());
        assert_eq!(incs(&f.blocks[4]), vec![ValueId(0)]);
    }

    /// The cell arm releases an object inline with no null test
    /// splitting the block: the retain lands ahead of the count read,
    /// not between it and the store that writes the count back.
    #[test]
    fn a_retain_stops_ahead_of_an_inline_release() {
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![
                    inst(None, InstKind::RcIncAny(v(0))),
                    inst(Some(1), InstKind::AnyIsNumber(v(0))),
                ],
                term: Terminator::CondBr {
                    cond: v(1),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![],
                term: Terminator::Ret(None),
            },
            Block {
                id: BlockId(2),
                insts: vec![
                    inst(Some(3), InstKind::Load(Type::I32, v(2), 0)),
                    inst(
                        Some(4),
                        InstKind::BinOp(torajs_core::ssa::BinOp::Sub, v(3), Operand::ConstI32(1)),
                    ),
                    inst(None, InstKind::Store(v(4), v(2), 0)),
                    inst(None, InstKind::Call(FuncId(0), vec![v(0)])),
                ],
                term: Terminator::Ret(None),
            },
        ];
        let mut m = module(
            vec![0, 2],
            &[Type::Any, Type::Bool, Type::Ptr, Type::I32, Type::I32],
            blocks,
        );
        assert_eq!(sink_number_retains(&mut m).retains_placed, 1);
        let b2 = &m.funcs[0].blocks[2].insts;
        assert!(matches!(b2[0].kind, InstKind::RcIncAny(_)));
        assert!(matches!(b2[1].kind, InstKind::Load(..)));
    }

    /// A retain ahead of a loop stays: sunk into the body it would run
    /// once per trip.
    #[test]
    fn a_retain_never_sinks_into_a_loop() {
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![inst(None, InstKind::RcIncAny(v(0)))],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(1),
                insts: vec![inst(None, InstKind::Call(FuncId(0), vec![v(0)]))],
                term: Terminator::CondBr {
                    cond: v(1),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(2),
                insts: vec![],
                term: Terminator::Ret(None),
            },
        ];
        let mut m = module(vec![0, 1], &[Type::Any, Type::Bool], blocks);
        let st = sink_number_retains(&mut m);
        assert_eq!(st, IncSinkStats::default());
        assert_eq!(incs(&m.funcs[0].blocks[0]), vec![ValueId(0)]);
    }
}
