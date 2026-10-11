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
//! argument register a relocated param still arrives in, except a block
//! that leaves the function, once the param's last read in it is past.

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
    // an entry that needs the frame is the save point already: no
    // dominator tree to build
    if needs_frame(&func.blocks[0], alloc, &arrivals) {
        return None;
    }
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
    let clobbers_arrival = func
        .blocks
        .iter()
        .filter(|b| pre.contains(&b.id.0))
        .any(|b| block_clobbers_arrival(b, alloc, &arrivals));
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

/// Whether a frameless block writes a register a relocated param still
/// arrives in while that param can be read afterwards. A block that
/// leaves the function (no successors) may reuse the register once the
/// param's last read in it has passed: the save point never runs after
/// it, so nothing downstream expects the param there. The defining inst
/// itself must not read the param either — the allocator never hands an
/// inst a result register equal to one of its operands, and emitters
/// rely on that. The usual case is a base case returning a fresh value
/// in x0 / d0, the first param's own register. Reads are judged by SSA
/// index: a fusion that emits an absorbed inst's operand reads at a
/// later inst must read them before it writes its result register.
fn block_clobbers_arrival(
    block: &torajs_core::ssa::Block,
    alloc: &Assignment,
    arrivals: &HashMap<u32, Reg>,
) -> bool {
    let terminal = successors(&block.term).next().is_none();
    block.insts.iter().enumerate().any(|(i, inst)| {
        let Some(r) = inst.result.filter(|r| alloc.contains(*r)) else {
            return false;
        };
        let home = alloc.of(r);
        arrivals
            .iter()
            .filter(|(_, reg)| **reg == home)
            .any(|(p, _)| !terminal || read_from(block, i, *p))
    })
}

/// Whether value `p` is read by the inst at index `i`, a later inst, or
/// the terminator.
fn read_from(block: &torajs_core::ssa::Block, i: usize, p: u32) -> bool {
    let mut hit = false;
    for inst in &block.insts[i..] {
        visit_value_operands(&inst.kind, |v| hit |= v.0 == p);
    }
    let term_reads = match &block.term {
        Terminator::Ret(Some(Operand::Value(v))) => Some(v.0),
        _ => None,
    };
    hit || term_reads == Some(p)
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

    /// %0 arrives in x0 and %1 in x1; %7 is homed in x0.
    fn arrival_case(insts: Vec<Inst>, term: Terminator) -> bool {
        let block = Block {
            id: BlockId(1),
            insts,
            term,
        };
        let homes = [
            (0, Reg::Gpr(Gpr::X19)),
            (1, Reg::Gpr(Gpr::X20)),
            (7, Reg::Gpr(Gpr::X0)),
        ];
        let alloc =
            Assignment::from_parts(homes.into_iter().collect(), HashMap::default(), 0, 0, true);
        let arrivals = HashMap::from_iter([(0, Reg::Gpr(Gpr::X0)), (1, Reg::Gpr(Gpr::X1))]);
        block_clobbers_arrival(&block, &alloc, &arrivals)
    }

    fn add1(r: u32, of: u32) -> Inst {
        inst(r, InstKind::BinOp(BinOp::Add, v(of), Operand::ConstI64(1)))
    }

    #[test]
    fn a_returning_block_may_reuse_a_dead_params_register() {
        assert!(!arrival_case(vec![add1(7, 1)], Terminator::Ret(Some(v(7)))));
    }

    #[test]
    fn a_param_read_at_or_after_the_def_keeps_its_register() {
        let later = vec![add1(7, 1), inst(8, InstKind::BinOp(BinOp::Add, v(7), v(0)))];
        assert!(arrival_case(later, Terminator::Ret(Some(v(8)))));
        assert!(arrival_case(vec![add1(7, 0)], Terminator::Ret(Some(v(7)))));
        assert!(arrival_case(vec![add1(7, 1)], Terminator::Ret(Some(v(0)))));
    }

    #[test]
    fn a_block_with_successors_keeps_every_arrival_register() {
        assert!(arrival_case(vec![add1(7, 1)], Terminator::Br(BlockId(2))));
    }

    /// `f(a, b) { if (a < 2) return b + 1; return f(a - 1, b) + a + b }`:
    /// both params cross the call, and the base case's result takes the
    /// return register, which is `a`'s argument register.
    #[test]
    fn a_base_case_computing_into_the_first_arg_register_runs_frameless() {
        let mut func = countdown(false);
        func.params.push(ValueId(6));
        func.values
            .extend([Type::I64, Type::I64, Type::I64].map(|ty| ValueInfo { ty, name: None }));
        func.blocks[1].insts = vec![add1(7, 6)];
        func.blocks[1].term = Terminator::Ret(Some(v(7)));
        let rec = &mut func.blocks[2];
        rec.insts[1] = inst(3, InstKind::Call(FuncId(0), vec![v(2), v(6)]));
        rec.insts
            .push(inst(8, InstKind::BinOp(BinOp::Add, v(4), v(6))));
        rec.term = Terminator::Ret(Some(v(8)));
        let alloc = allocate_linear_scan(&func);
        assert_eq!(
            alloc.of(ValueId(7)),
            Reg::Gpr(Gpr::X0),
            "the shape under test"
        );
        let sw = plan(&func, &alloc).expect("the base case needs no frame");
        assert_eq!(sw.save, BlockId(2));
    }

    #[test]
    fn a_call_in_the_entry_keeps_the_prologue_there() {
        let func = countdown(true);
        let alloc = allocate_linear_scan(&func);
        assert!(plan(&func, &alloc).is_none());
    }
}
