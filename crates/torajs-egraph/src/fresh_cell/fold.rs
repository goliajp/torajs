//! The path's control flow: predecessor counts over the reachable
//! cfg, the constant evaluation that decides a guard's branch from a
//! forwarded header word, and the clean-up a decided branch leaves.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Operand, Terminator, ValueId, visit_value_operands,
};

use crate::branch_fold::point_eval::successors;
use crate::slot_forward::resolve;

/// Predecessors of every block, counting only blocks reachable from
/// the entry: a decided branch leaves its dropped arm in the layout
/// with its terminator intact.
pub(super) fn reachable_preds(func: &Function) -> Vec<Vec<BlockId>> {
    let mut preds: Vec<Vec<BlockId>> = vec![Vec::new(); func.blocks.len()];
    for b in reachable(func) {
        for s in successors(&func.blocks[b].term) {
            preds[s.0 as usize].push(BlockId(b as u32));
        }
    }
    preds
}

fn reachable(func: &Function) -> Vec<usize> {
    let mut seen = vec![false; func.blocks.len()];
    let mut stack = vec![0usize];
    let mut out = Vec::new();
    while let Some(b) = stack.pop() {
        if std::mem::replace(&mut seen[b], true) {
            continue;
        }
        out.push(b);
        for s in successors(&func.blocks[b].term) {
            stack.push(s.0 as usize);
        }
    }
    out
}

/// Pure chains longer than a guard's `and; icmp` are not the shape.
const MAX_DEPTH: u32 = 8;

/// `op` as a constant, evaluating integer and boolean arithmetic and
/// compares over constant operands, a null pointer among them (the
/// nullish test in front of releasing a field the factory wrote null
/// into). Every value whose def was evaluated is
/// pushed onto `chain`, so the defs can go once the branch that read
/// them is decided.
pub(super) fn const_eval(
    func: &Function,
    defs: &HashMap<ValueId, (usize, usize)>,
    replace: &HashMap<ValueId, Operand>,
    op: &Operand,
    chain: &mut Vec<ValueId>,
) -> Option<Operand> {
    eval(func, defs, replace, op, chain, 0)
}

fn eval(
    func: &Function,
    defs: &HashMap<ValueId, (usize, usize)>,
    replace: &HashMap<ValueId, Operand>,
    op: &Operand,
    chain: &mut Vec<ValueId>,
    depth: u32,
) -> Option<Operand> {
    let op = resolve(op, replace);
    let Operand::Value(v) = op else {
        return Some(op);
    };
    if depth == MAX_DEPTH {
        return None;
    }
    let &(bi, ii) = defs.get(&v)?;
    let sub =
        |o: &Operand, chain: &mut Vec<ValueId>| eval(func, defs, replace, o, chain, depth + 1);
    let out = match &func.blocks[bi].insts[ii].kind {
        InstKind::BinOp(o, a, b) => {
            let a = sub(a, chain)?;
            let b = sub(b, chain)?;
            fold_binop(*o, a, b)?
        }
        InstKind::ICmp(p, a, b) => {
            let a = sub(a, chain)?;
            // a null against a global's address: never equal
            let global = match resolve(b, replace) {
                Operand::Value(g) => defs.get(&g).and_then(|&(bi, ii)| {
                    matches!(func.blocks[bi].insts[ii].kind, InstKind::GlobalRef(_)).then_some(g)
                }),
                _ => None,
            };
            match (global, p) {
                (Some(g), IPred::Eq | IPred::Ne) if a == Operand::ConstPtrNull => {
                    chain.push(g);
                    Operand::ConstBool(matches!(p, IPred::Ne))
                }
                _ => {
                    let b = sub(b, chain)?;
                    fold_icmp(*p, a, b)?
                }
            }
        }
        _ => return None,
    };
    chain.push(v);
    Some(out)
}

fn fold_binop(op: BinOp, a: Operand, b: Operand) -> Option<Operand> {
    match (a, b) {
        (Operand::ConstBool(x), Operand::ConstBool(y)) => Some(Operand::ConstBool(match op {
            BinOp::And => x & y,
            BinOp::Or => x | y,
            BinOp::Xor => x ^ y,
            _ => return None,
        })),
        (Operand::ConstI64(x), Operand::ConstI64(y)) => Some(Operand::ConstI64(bits(op, x, y)?)),
        // the low 32 bits of the wrapping i64 result are the i32 result
        (Operand::ConstI32(x), Operand::ConstI32(y)) => {
            Some(Operand::ConstI32(bits(op, x as i64, y as i64)? as i32))
        }
        _ => None,
    }
}

fn bits(op: BinOp, x: i64, y: i64) -> Option<i64> {
    Some(match op {
        BinOp::Add => x.wrapping_add(y),
        BinOp::Sub => x.wrapping_sub(y),
        BinOp::Mul => x.wrapping_mul(y),
        BinOp::And => x & y,
        BinOp::Or => x | y,
        BinOp::Xor => x ^ y,
        _ => return None,
    })
}

fn fold_icmp(pred: IPred, a: Operand, b: Operand) -> Option<Operand> {
    // sign extension keeps both the signed and the unsigned order of
    // two i32 values
    let (x, y) = match (a, b) {
        (Operand::ConstPtrNull, Operand::ConstPtrNull) => (0, 0),
        (Operand::ConstI64(x), Operand::ConstI64(y)) => (x, y),
        (Operand::ConstI32(x), Operand::ConstI32(y)) => (x as i64, y as i64),
        _ => return None,
    };
    Some(Operand::ConstBool(match pred {
        IPred::Eq => x == y,
        IPred::Ne => x != y,
        IPred::Slt => x < y,
        IPred::Sgt => x > y,
        IPred::Sle => x <= y,
        IPred::Sge => x >= y,
        IPred::Ult => (x as u64) < (y as u64),
        IPred::Uge => (x as u64) >= (y as u64),
    }))
}

/// Delete the evaluated compare chains' defs that nothing reads any
/// more, innermost use first.
pub(super) fn sweep_dead_chain(func: &mut Function, chain: &[ValueId]) {
    let chain: HashSet<ValueId> = chain.iter().copied().collect();
    loop {
        let uses = used_values(func);
        let mut removed = false;
        for block in func.blocks.iter_mut() {
            block.insts.retain(|inst| {
                let dead = inst
                    .result
                    .is_some_and(|r| chain.contains(&r) && !uses.contains(&r));
                removed |= dead;
                !dead
            });
        }
        if !removed {
            return;
        }
    }
}

fn used_values(func: &Function) -> HashSet<ValueId> {
    let mut used = HashSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                used.insert(v);
            });
        }
        match &block.term {
            Terminator::Ret(Some(Operand::Value(v)))
            | Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            } => {
                used.insert(*v);
            }
            _ => {}
        }
    }
    used
}

/// Empty every block the entry no longer reaches and pin its
/// terminator, the shape elaboration leaves such blocks in.
pub(super) fn detach_unreachable(func: &mut Function) {
    let live: HashSet<usize> = reachable(func).into_iter().collect();
    for (bi, block) in func.blocks.iter_mut().enumerate() {
        if !live.contains(&bi) {
            block.insts.clear();
            block.term = Terminator::Unreachable;
        }
    }
}
