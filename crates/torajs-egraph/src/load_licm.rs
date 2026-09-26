//! Array header-field load hoisting — the one family of `Load` the
//! egraph's LICM (`optimize_licm.rs`) may not move, because it has no
//! alias model: `for (i = 0; i < src.length; i++) dst.push(src[i])`
//! re-read `src`'s length and data pointer on every iteration.
//!
//! The alias model is the allocation-site one `interval::elem` already
//! keeps: a live tracked array is a fresh allocation whose address
//! never leaves the function's SSA, so the only instructions that can
//! touch its cell are the ones naming it. Its header — length (+8),
//! capacity/head (+16), props (+24), data pointer (+32) — is a
//! separate slice from its element slots (HotSpot C2's offset alias
//! slices, JSC B3's heap ranges): a slot write goes through the data
//! pointer at or past +40 and never lands in [0, 40).
//!
//! For each natural loop, innermost first, a `Load(_, p, 8 | 16 | 32)`
//! whose `p` names live array A and is defined outside the loop moves
//! to the tail of `idom(header)` when nothing in the loop body writes
//! A's header: no `Store` through a name of A, and no `Call` /
//! `CallIndirect` taking a name of A or of its data pointer (push,
//! reserve, shift, splice, a `length` write, a user function — all of
//! them call with A). The refcount / flags word (+0 / +4) is out of
//! scope: rc helpers and the cycle collector write it.
//!
//! The moved load runs on paths that never enter the loop too. That
//! is safe: A comes from `__torajs_arr_alloc*`, which never answers
//! null and writes the whole header before returning, and the cell
//! never moves (a grow swaps the data buffer, not the cell); the only
//! instruction that frees it is a drop naming A, which the loop body
//! does not contain, and one before `idom(header)` would already be a
//! use-after-free in the loop itself.
//!
//! `TORAJS_LOAD_LICM_OFF=1` skips, `TORAJS_LOAD_LICM_STATS=1` counts.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Function, InstKind, Module, Operand, Terminator, ValueId};

use crate::interval::elem::{ArrayClasses, array_classes};
use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;

#[derive(Debug, Default)]
pub struct LoadLicmStats {
    /// Header-field loads moved out of a loop (a load hoisted through
    /// two nested loops counts twice).
    pub loads_hoisted: u32,
}

const ARR_LEN_OFF: u64 = 8;
const ARR_CAP_HEAD_OFF: u64 = 16;
const ARR_DATA_PTR_OFF: u64 = 32;

pub fn hoist_header_loads(module: &mut Module) -> LoadLicmStats {
    let names: Vec<String> = module.funcs.iter().map(|f| f.name.clone()).collect();
    let mut stats = LoadLicmStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        if let Some(classes) = array_classes(func, &names) {
            hoist_fn(func, &classes, &mut stats);
        }
    }
    stats
}

fn hoist_fn(func: &mut Function, classes: &ArrayClasses, stats: &mut LoadLicmStats) {
    if classes.live.is_empty() {
        return;
    }
    let dom = DominatorTree::compute(func);
    let mut loops = LoopAnalysis::compute(func, &dom).loops().to_vec();
    // inner first: an inner loop's preheader is in the outer body, so
    // what moves out of the inner loop is judged again by the outer
    loops.sort_by_key(|l| l.body.len());
    let mut def_block: HashMap<ValueId, BlockId> = HashMap::new();
    for (bi, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                def_block.insert(r, BlockId(bi as u32));
            }
        }
    }
    let n = func.blocks.len();
    for lp in &loops {
        let Some(pre) = dom.immediate_dominator(lp.header) else {
            continue;
        };
        if pre == lp.header || lp.body.contains(&pre) || pre.0 as usize >= n {
            continue;
        }
        let body: HashSet<BlockId> = lp.body.iter().copied().collect();
        if !sole_entry(func, pre, lp.header, &body) {
            continue;
        }
        let dirty = written_headers(func, &body, classes);
        for &b in &lp.body {
            let insts = std::mem::take(&mut func.blocks[b.0 as usize].insts);
            let (moved, kept): (Vec<_>, Vec<_>) = insts.into_iter().partition(|inst| {
                let InstKind::Load(_, Operand::Value(p), off) = &inst.kind else {
                    return false;
                };
                matches!(*off, ARR_LEN_OFF | ARR_CAP_HEAD_OFF | ARR_DATA_PTR_OFF)
                    && classes
                        .class
                        .get(p)
                        .is_some_and(|a| classes.live.contains(a) && !dirty.contains(a))
                    && def_block.get(p).is_some_and(|d| !body.contains(d))
            });
            func.blocks[b.0 as usize].insts = kept;
            for inst in moved {
                if let Some(r) = inst.result {
                    def_block.insert(r, pre);
                }
                func.blocks[pre.0 as usize].insts.push(inst);
                stats.loads_hoisted += 1;
            }
        }
    }
}

/// `pre` jumps straight to `header` and is its only way in from
/// outside the loop. Otherwise a block between them (a diamond's arm,
/// `if (c) a.length = 0` just before the loop) could write the header
/// after the hoisted load has read it.
fn sole_entry(func: &Function, pre: BlockId, header: BlockId, body: &HashSet<BlockId>) -> bool {
    matches!(func.blocks[pre.0 as usize].term, Terminator::Br(h) if h == header)
        && func.blocks.iter().enumerate().all(|(bi, block)| {
            let b = BlockId(bi as u32);
            b == pre
                || body.contains(&b)
                || !match &block.term {
                    Terminator::Br(t) => *t == header,
                    Terminator::CondBr {
                        then_blk, else_blk, ..
                    } => *then_blk == header || *else_blk == header,
                    Terminator::Ret(_) | Terminator::Unreachable => false,
                }
        })
}

/// The tracked arrays whose header some instruction in `body` may
/// write: a store through a name of the array, or a call handed a
/// name of the array or of its data pointer.
fn written_headers(
    func: &Function,
    body: &HashSet<BlockId>,
    classes: &ArrayClasses,
) -> HashSet<ValueId> {
    let mut dirty = HashSet::new();
    let mut named = |op: &Operand, with_base: bool| {
        if let Operand::Value(v) = op {
            dirty.extend(classes.class.get(v).copied());
            if with_base {
                dirty.extend(classes.base.get(v).copied());
            }
        }
    };
    for &b in body {
        for inst in &func.blocks[b.0 as usize].insts {
            match &inst.kind {
                // a slot write through the data pointer stays past +40
                InstKind::Store(_, p, _)
                | InstKind::StoreDyn(_, p, _)
                | InstKind::StoreDynScaled8(_, p, _) => named(p, false),
                InstKind::Call(_, args) => args.iter().for_each(|a| named(a, true)),
                InstKind::CallIndirect(_, callee, args) => {
                    named(callee, true);
                    args.iter().for_each(|a| named(a, true));
                }
                _ => {}
            }
        }
    }
    dirty
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, FuncId, IPred, Inst, Type, ValueInfo};

    fn decl(name: &str) -> Function {
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Ptr,
            blocks: vec![],
            values: vec![],
            current_origin: None,
        }
    }
    fn mk(result: Option<u32>, kind: InstKind) -> Inst {
        Inst {
            result: result.map(ValueId),
            kind,
            origin: None,
        }
    }
    fn v(i: u32) -> Operand {
        Operand::Value(ValueId(i))
    }

    /// b0: %0 = arr_alloc(8) · `entry` · Br b1 | b1 (header): %1 = len of
    /// %0 · %2 = 0 < %1 · CondBr b2 / b3 | b2 (body): %3 = data of %0 ·
    /// `body` · Br b1 | b3: Ret — `for (…; i < a.length; …) a[i]`.
    fn walk(entry_term: Terminator, body: Vec<Inst>) -> Module {
        let mut body_insts = vec![mk(Some(3), InstKind::Load(Type::Ptr, v(0), 32))];
        body_insts.extend(body);
        let mut blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![mk(
                    Some(0),
                    InstKind::Call(FuncId(0), vec![Operand::ConstI64(8)]),
                )],
                term: entry_term,
            },
            Block {
                id: BlockId(1),
                insts: vec![
                    mk(Some(1), InstKind::Load(Type::I64, v(0), 8)),
                    mk(
                        Some(2),
                        InstKind::ICmp(IPred::Slt, Operand::ConstI64(0), v(1)),
                    ),
                ],
                term: Terminator::CondBr {
                    cond: v(2),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            },
            Block {
                id: BlockId(2),
                insts: body_insts,
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(3),
                insts: vec![],
                term: Terminator::Ret(None),
            },
        ];
        // a diamond arm between the entry and the header
        if !matches!(blocks[0].term, Terminator::Br(_)) {
            blocks.push(Block {
                id: BlockId(4),
                insts: vec![],
                term: Terminator::Br(BlockId(1)),
            });
        }
        let ty = |t| ValueInfo { ty: t, name: None };
        Module {
            funcs: vec![
                decl("__torajs_arr_alloc"),
                decl("__torajs_arr_push"),
                Function {
                    name: "main".into(),
                    params: vec![],
                    ret: Type::I64,
                    blocks,
                    values: vec![
                        ty(Type::Ptr),
                        ty(Type::I64),
                        ty(Type::Bool),
                        ty(Type::Ptr),
                        ty(Type::Ptr),
                    ],
                    current_origin: None,
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn a_read_only_walk_reads_the_header_once() {
        let mut m = walk(Terminator::Br(BlockId(1)), vec![]);
        assert_eq!(hoist_header_loads(&mut m).loads_hoisted, 2);
        let f = &m.funcs[2];
        assert_eq!(f.blocks[0].insts.len(), 3);
        assert_eq!(f.blocks[1].insts.len(), 1);
        assert!(f.blocks[2].insts.is_empty());
    }

    #[test]
    fn a_push_onto_the_walked_array_keeps_both_loads() {
        let push = mk(
            Some(4),
            InstKind::Call(FuncId(1), vec![v(0), Operand::ConstI64(1)]),
        );
        let mut m = walk(Terminator::Br(BlockId(1)), vec![push]);
        assert_eq!(hoist_header_loads(&mut m).loads_hoisted, 0);
    }

    #[test]
    fn a_length_store_in_the_loop_keeps_both_loads() {
        let store = mk(None, InstKind::Store(Operand::ConstI64(1), v(0), 8));
        let mut m = walk(Terminator::Br(BlockId(1)), vec![store]);
        assert_eq!(hoist_header_loads(&mut m).loads_hoisted, 0);
    }

    #[test]
    fn a_slot_write_through_the_data_pointer_does_not_stop_it() {
        let slot = mk(None, InstKind::Store(Operand::ConstI64(1), v(3), 0));
        let mut m = walk(Terminator::Br(BlockId(1)), vec![slot]);
        assert_eq!(hoist_header_loads(&mut m).loads_hoisted, 2);
    }

    #[test]
    fn a_second_way_into_the_header_keeps_both_loads() {
        let diamond = Terminator::CondBr {
            cond: Operand::ConstBool(true),
            then_blk: BlockId(1),
            else_blk: BlockId(4),
        };
        let mut m = walk(diamond, vec![]);
        assert_eq!(hoist_header_loads(&mut m).loads_hoisted, 0);
    }
}
