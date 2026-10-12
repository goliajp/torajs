//! Cloning loop regions under lane renaming, and the clone a lane
//! finishes its iteration in alone.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
    rewrite_value_operands, visit_value_operands,
};

use super::Plan;

/// One lane finishing alone: the inner loop, post and latch cloned on
/// the original cells, the latch's back edge sent to `next`. Values
/// `pre` computed come from this lane's copies; every other non-cell
/// def gets a fresh id. Returns each block's clone; the entry is the
/// inner header's.
pub(super) fn tail_clone(
    func: &mut Function,
    p: &Plan,
    cells: &HashSet<ValueId>,
    lane: &HashMap<ValueId, ValueId>,
    next: BlockId,
) -> HashMap<BlockId, BlockId> {
    let blocks: Vec<BlockId> = p
        .inner_order()
        .into_iter()
        .chain(p.post.iter().copied())
        .collect();
    let edge = |t: BlockId, bmap: &HashMap<BlockId, BlockId>| match bmap.get(&t) {
        _ if t == p.header => next,
        Some(&c) => c,
        None => t,
    };
    clone_region(func, p, cells, lane, &blocks, edge)
}

/// `blocks` cloned on the original cells the way `tail_clone` clones
/// them, each edge sent where `edge` maps it given the clones. Returns
/// each block's clone.
pub(super) fn clone_region(
    func: &mut Function,
    p: &Plan,
    cells: &HashSet<ValueId>,
    lane: &HashMap<ValueId, ValueId>,
    blocks: &[BlockId],
    edge: impl Fn(BlockId, &HashMap<BlockId, BlockId>) -> BlockId,
) -> HashMap<BlockId, BlockId> {
    let mut map: HashMap<ValueId, ValueId> = lane
        .iter()
        // only what `pre` computed: every cell, the inner loop's
        // temporaries included, is read and written under its own id
        .filter(|(v, _)| **v != p.iv && !cells.contains(v) && !p.escaping.contains(v))
        .map(|(v, n)| (*v, *n))
        .collect();
    let bmap: HashMap<BlockId, BlockId> = blocks.iter().map(|b| (*b, func.add_block())).collect();
    for &b in blocks {
        for inst in &func.blocks[b.0 as usize].insts.clone() {
            if let Some(r) = inst.result
                && !cells.contains(&r)
                && !p.escaping.contains(&r)
            {
                let fresh = clone_value(func, r);
                map.insert(r, fresh);
            }
        }
    }
    for &b in blocks {
        let src = func.blocks[b.0 as usize].clone();
        let nb = bmap[&b];
        for inst in &src.insts {
            let mut kind = inst.kind.clone();
            rewrite_value_operands(&mut kind, |v| map.get(&v).copied().unwrap_or(v));
            func.blocks[nb.0 as usize].insts.push(Inst {
                result: inst.result.map(|r| map.get(&r).copied().unwrap_or(r)),
                kind,
                origin: inst.origin,
            });
        }
        let mut term = src.term.clone();
        let fix = |v: ValueId| map.get(&v).copied().unwrap_or(v);
        match &mut term {
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) => *v = fix(*v),
            _ => {}
        }
        retarget(&mut term, |t| edge(t, &bmap), p.header, None);
        func.blocks[nb.0 as usize].term = term;
    }
    bmap
}

/// Values every def of which is a `Copy`: the cells clones keep under
/// their own id.
pub(super) fn cell_set(func: &Function) -> HashSet<ValueId> {
    let mut copy: HashSet<ValueId> = HashSet::default();
    let mut other: HashSet<ValueId> = HashSet::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                if matches!(inst.kind, InstKind::Copy(..)) {
                    copy.insert(r);
                } else {
                    other.insert(r);
                }
            }
        }
    }
    copy.difference(&other).copied().collect()
}

/// Values read anywhere but in `blocks`: the lane cells a finishing
/// lane needs copied back (one only `pre` reads is dead there).
pub(super) fn reads_outside(func: &Function, blocks: &[BlockId]) -> HashSet<ValueId> {
    let mut out = HashSet::default();
    for block in func.blocks.iter().filter(|b| !blocks.contains(&b.id)) {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                out.insert(v);
            });
        }
        if let Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) = block.term
        {
            out.insert(v);
        }
    }
    out
}

/// Map each successor through `f`; an edge to `from` goes to `to`.
pub(super) fn retarget(
    term: &mut Terminator,
    f: impl Fn(BlockId) -> BlockId,
    from: BlockId,
    to: Option<BlockId>,
) {
    let one = |t: &mut BlockId| {
        *t = match to {
            Some(to) if *t == from => to,
            _ => f(*t),
        }
    };
    match term {
        Terminator::Br(t) => one(t),
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => {
            one(then_blk);
            one(else_blk);
        }
        Terminator::Ret(_) | Terminator::Unreachable => {}
    }
}

/// Clone `inst` into `dst` under lane renaming: operands through
/// `map`, the result to its lane cell or a fresh value.
pub(super) fn clone_into(
    func: &mut Function,
    dst: BlockId,
    inst: &Inst,
    map: &mut HashMap<ValueId, ValueId>,
) {
    let mut kind = inst.kind.clone();
    rewrite_value_operands(&mut kind, |v| map.get(&v).copied().unwrap_or(v));
    let r = inst.result.expect("planned regions define values");
    let result = match map.get(&r) {
        Some(&cell) => cell,
        None => {
            let fresh = clone_value(func, r);
            map.insert(r, fresh);
            fresh
        }
    };
    func.blocks[dst.0 as usize].insts.push(Inst {
        result: Some(result),
        kind,
        origin: inst.origin,
    });
}

pub(super) fn clone_value(func: &mut Function, like: ValueId) -> ValueId {
    let ty = func.values[like.0 as usize].ty;
    new_value(func, ty)
}

pub(super) fn new_value(func: &mut Function, ty: Type) -> ValueId {
    let id = ValueId(func.values.len() as u32);
    func.values.push(ValueInfo { ty, name: None });
    id
}

pub(super) fn push(func: &mut Function, b: BlockId, result: ValueId, kind: InstKind) {
    func.blocks[b.0 as usize].insts.push(Inst {
        result: Some(result),
        kind,
        origin: None,
    });
}
