//! Shrink-wrapping: place the prologue at the start of the nearest
//! block that dominates every block needing the frame, instead of at
//! function entry, so an early return that needs none (a recursion's
//! base case, a guard's bail-out) runs without saving and restoring
//! callee-saved registers. The single-save-point form of LLVM's
//! ShrinkWrap, run as it is there: after register allocation.
//!
//! A block needs the frame when it calls, carves a stack slot, or
//! touches a value homed in a callee-saved register or a spill slot.
//! The blocks the save point does not dominate run before it; they
//! are emitted with a bare `ret`. A param the allocator relocated out
//! of its argument register (the entry move the prologue is followed
//! by) is read from that argument register there instead.
//!
//! The save point is taken only when the blocks before it can never
//! run after it: every predecessor of the save point runs before it,
//! and no block it dominates branches back out of its region. That
//! also keeps every callee-saved value's whole live range inside the
//! region. The blocks before it may not define a value into an
//! argument register a relocated param still arrives in.

use torajs_core::ssa::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{BlockId, Function, InstKind, Operand, Terminator, visit_value_operands};

use crate::reg::{Reg, aapcs64};
use crate::regalloc::{Assignment, inst_emits_bl};

pub(crate) struct ShrinkWrap {
    /// The block whose start carries the prologue and entry moves.
    pub save: BlockId,
    /// Blocks that run before the prologue (by id).
    pub pre: HashSet<u32>,
    /// `alloc` with each relocated param homed in its argument register.
    pub pre_alloc: Assignment,
}

pub(crate) fn plan(func: &Function, alloc: &Assignment) -> Option<ShrinkWrap> {
    if !alloc.stack_param_entry_loads.is_empty() || func.blocks.len() < 2 {
        return None;
    }
    let arrivals: HashMap<u32, Reg> = func
        .params
        .iter()
        .filter(|p| alloc.contains(**p))
        .filter_map(|p| {
            let home = alloc.of(*p);
            let (src, _) = alloc.param_entry_moves.iter().find(|(_, d)| *d == home)?;
            Some((p.0, *src))
        })
        .collect();
    let dom = DominatorTree::compute(func);
    let needs: Vec<BlockId> = func
        .blocks
        .iter()
        .filter(|b| dom.is_reachable(b.id) && needs_frame(b, alloc, &arrivals))
        .map(|b| b.id)
        .collect();
    let save = needs
        .iter()
        .copied()
        .reduce(|a, b| common_dominator(&dom, a, b))?;
    let entry = func.blocks[0].id;
    if save == entry {
        return None;
    }
    let pre: HashSet<u32> = func
        .blocks
        .iter()
        .filter(|b| dom.is_reachable(b.id) && !dom.dominates(save, b.id))
        .map(|b| b.id.0)
        .collect();
    for b in &func.blocks {
        let in_pre = pre.contains(&b.id.0);
        for s in successors(&b.term) {
            let into_pre = pre.contains(&s.0);
            // the save point is entered only from before it, and the
            // region never branches back out
            if (s == save && !in_pre) || (into_pre && !in_pre && dom.is_reachable(b.id)) {
                return None;
            }
        }
    }
    let arrival_regs: Vec<Reg> = arrivals.values().copied().collect();
    let clobbers_arrival = func
        .blocks
        .iter()
        .filter(|b| pre.contains(&b.id.0))
        .flat_map(|b| &b.insts)
        .filter_map(|i| i.result)
        .any(|r| alloc.contains(r) && arrival_regs.contains(&alloc.of(r)));
    if clobbers_arrival {
        return None;
    }
    let homes: Vec<(u32, Reg)> = arrivals.into_iter().collect();
    Some(ShrinkWrap {
        save,
        pre,
        pre_alloc: alloc.with_homes(&homes),
    })
}

fn needs_frame(
    block: &torajs_core::ssa::Block,
    alloc: &Assignment,
    arrivals: &HashMap<u32, Reg>,
) -> bool {
    let framed = |v: torajs_core::ssa::ValueId| alloc.contains(v) && frame_home(alloc.of(v));
    let mut hit = false;
    for inst in &block.insts {
        if inst_emits_bl(&inst.kind)
            || matches!(inst.kind, InstKind::Alloca(_) | InstKind::AllocaBytes(_))
            || inst.result.is_some_and(framed)
        {
            return true;
        }
        visit_value_operands(&inst.kind, |v| {
            hit |= framed(v) && !arrivals.contains_key(&v.0);
        });
    }
    let term_reads = match &block.term {
        Terminator::Ret(Some(Operand::Value(v)))
        | Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        } => Some(*v),
        _ => None,
    };
    hit || term_reads.is_some_and(|v| framed(v) && !arrivals.contains_key(&v.0))
}

fn frame_home(r: Reg) -> bool {
    match r {
        Reg::Gpr(g) => aapcs64::CALLEE_SAVED_SCRATCH.contains(&g),
        Reg::Fpr(f) => aapcs64::FP_CALLEE_SAVED_SCRATCH.contains(&f),
        Reg::SpillGpr(_) | Reg::SpillFpr(_) => true,
    }
}

fn common_dominator(dom: &DominatorTree, a: BlockId, b: BlockId) -> BlockId {
    let mut up = a;
    loop {
        if dom.dominates(up, b) {
            return up;
        }
        up = dom
            .immediate_dominator(up)
            .expect("the entry dominates every reachable block");
    }
}

fn successors(term: &Terminator) -> impl Iterator<Item = BlockId> {
    let out = match term {
        Terminator::Br(b) => [Some(*b), None],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => [Some(*then_blk), Some(*else_blk)],
        Terminator::Ret(_) | Terminator::Unreachable => [None, None],
    };
    out.into_iter().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enc::cmp_imm;
    use crate::linear_scan::allocate_linear_scan;
    use crate::reg::Gpr;
    use torajs_core::ssa::{BinOp, Block, FuncId, IPred, Inst, Type, ValueId, ValueInfo};

    fn inst(r: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        }
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    /// `f(n) { if (n < 2) return n; return f(n - 1) + n }` — `n` crosses
    /// the call, so it is relocated to a callee-saved register.
    fn countdown(call_in_entry: bool) -> Function {
        let mut entry = vec![inst(
            1,
            InstKind::ICmp(IPred::Slt, v(0), Operand::ConstI64(2)),
        )];
        if call_in_entry {
            entry.insert(0, inst(5, InstKind::Call(FuncId(0), vec![v(0)])));
        }
        Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::I64,
            values: [
                Type::I64,
                Type::Bool,
                Type::I64,
                Type::I64,
                Type::I64,
                Type::I64,
            ]
            .into_iter()
            .map(|ty| ValueInfo { ty, name: None })
            .collect(),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: entry,
                    term: Terminator::CondBr {
                        cond: v(1),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: vec![],
                    term: Terminator::Ret(Some(v(0))),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![
                        inst(2, InstKind::BinOp(BinOp::Sub, v(0), Operand::ConstI64(1))),
                        inst(3, InstKind::Call(FuncId(0), vec![v(2)])),
                        inst(4, InstKind::BinOp(BinOp::Add, v(3), v(0))),
                    ],
                    term: Terminator::Ret(Some(v(4))),
                },
            ],
            current_origin: None,
        }
    }

    #[test]
    fn base_case_runs_before_the_prologue() {
        let func = countdown(false);
        let alloc = allocate_linear_scan(&func);
        let sw = plan(&func, &alloc).expect("the recursive arm is the save point");
        assert_eq!(sw.save, BlockId(2));
        assert_eq!(sw.pre, HashSet::from_iter([0, 1]));
        assert_eq!(sw.pre_alloc.of(ValueId(0)), Reg::Gpr(Gpr::X0));
        let compiled = crate::compile::compile_function(&func);
        let first = u32::from_le_bytes(compiled.bytes[..4].try_into().unwrap());
        assert_eq!(
            first,
            cmp_imm(Gpr::X0, 2),
            "entry compares n in x0, no prologue"
        );
    }

    #[test]
    fn a_call_in_the_entry_keeps_the_prologue_there() {
        let func = countdown(true);
        let alloc = allocate_linear_scan(&func);
        assert!(plan(&func, &alloc).is_none());
    }
}
