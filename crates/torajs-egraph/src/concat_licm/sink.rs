//! The body's reference to a hoisted cell, minted where the concat
//! stood, moves down to the first instruction on each path that may
//! take it. The loop holds its own reference until an exit releases
//! it, so the cell stays alive whatever the body does before the
//! taker: the retain can pass any instruction that does not name the
//! cell, and any plain read of it.
//!
//! Two shapes fall out:
//!
//! - a release of the cell met first (a throw exit's close, the exit
//!   edge's own release of the loop's reference) cancels against the
//!   retain, so a path that never stores the cell touches no header;
//! - a slot store that first releases the slot's old cell,
//!   `old = load slot; str_drop(old); rc_inc h; store h, slot`,
//!   becomes the setter idiom `if (old != h) { str_drop(old); rc_inc
//!   h }; store h, slot`: a slot already holding the cell is left as
//!   it is, which in a ring the loop fills with the one cell is every
//!   store after the first lap.

use crate::id_hash::HashSet;

use torajs_core::ssa::{
    BlockId, FuncId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
    visit_value_operands,
};

use super::retarget;
use crate::cfg_edit::new_block;
use crate::float_demote::cfg::successors;
use crate::float_demote::push_value;

/// Where the retain of `h` ends up on each path from `header`.
#[derive(Default)]
struct Sink {
    /// Blocks reached with the retain still pending at their head.
    at_top: HashSet<BlockId>,
    /// Edges whose source reached its end with the retain pending.
    open: Vec<(BlockId, BlockId)>,
    /// (block, index): the retain goes before this instruction.
    before: Vec<(BlockId, usize)>,
    /// (block, index): this release of `h` cancels the retain.
    cancel: Vec<(BlockId, usize)>,
}

/// The blocks this adds inside the loop join `body`, so the next
/// cell's walk sees the loop as it now is.
pub(super) fn sink_minted(
    func: &mut Function,
    body: &mut HashSet<BlockId>,
    header: BlockId,
    h: ValueId,
    str_drop: FuncId,
) {
    let retain = InstKind::RcInc(Operand::Value(h));
    let Some(start) = func.blocks[header.0 as usize]
        .insts
        .iter()
        .position(|i| i.kind == retain)
    else {
        return;
    };
    func.blocks[header.0 as usize].insts.remove(start);
    let plan = walk(func, body, header, start, h, str_drop);
    // open edges first: their sources hold no other edit
    for (from, to) in &plan.open {
        let e = new_block(func);
        func.blocks[e.0 as usize].insts = vec![inst(retain.clone())];
        func.blocks[e.0 as usize].term = Terminator::Br(*to);
        retarget(&mut func.blocks[from.0 as usize].term, *to, e);
        if body.contains(to) {
            body.insert(e);
        }
    }
    for (b, i) in plan.cancel {
        func.blocks[b.0 as usize].insts.remove(i);
    }
    for (b, i) in plan.before {
        if let Some([tail, swap]) = swap_unless_same(func, b, i, h, str_drop) {
            body.extend([tail, swap]);
        } else {
            func.blocks[b.0 as usize]
                .insts
                .insert(i, inst(retain.clone()));
        }
    }
}

fn walk(
    func: &Function,
    body: &HashSet<BlockId>,
    header: BlockId,
    start: usize,
    h: ValueId,
    str_drop: FuncId,
) -> Sink {
    let mut preds = vec![0u32; func.blocks.len()];
    for block in &func.blocks {
        for s in successors(&block.term) {
            preds[s.0 as usize] += 1;
        }
    }
    let release = InstKind::Call(str_drop, vec![Operand::Value(h)]);
    let mut plan = Sink::default();
    let mut work = vec![(header, start)];
    let mut pending: Vec<(BlockId, BlockId)> = Vec::new();
    while let Some((b, from)) = work.pop() {
        let insts = &func.blocks[b.0 as usize].insts;
        match insts[from..].iter().position(|i| takes(&i.kind, h)) {
            Some(k) if insts[from + k].kind == release => plan.cancel.push((b, from + k)),
            Some(k) => plan.before.push((b, from + k)),
            None => pending.extend(successors(&func.blocks[b.0 as usize].term).map(|s| (b, s))),
        }
        // a block every predecessor reaches with the retain pending
        // takes it at its head; one that some other path enters gets
        // it on each pending edge
        let mut rest = Vec::new();
        for (from, to) in pending.drain(..) {
            let n = plan
                .open
                .iter()
                .chain(rest.iter())
                .filter(|(_, t)| *t == to)
                .count()
                + 1;
            if n as u32 == preds[to.0 as usize]
                && (body.contains(&to)
                    || releases_first(&func.blocks[to.0 as usize].insts, &release, h))
            {
                plan.open.retain(|(_, t)| *t != to);
                rest.retain(|(_, t)| *t != to);
                if plan.at_top.insert(to) {
                    work.push((to, 0));
                }
            } else {
                rest.push((from, to));
            }
        }
        plan.open.extend(rest);
    }
    plan
}

/// An instruction that may take the reference: anything naming `h`
/// other than a load from the cell or a compare of its address.
fn takes(kind: &InstKind, h: ValueId) -> bool {
    let mut names = false;
    visit_value_operands(kind, |v| names |= v == h);
    names
        && !matches!(
            kind,
            InstKind::Load(..)
                | InstKind::LoadDyn(..)
                | InstKind::LoadDynScaled8(..)
                | InstKind::LoadU8Dyn(..)
                | InstKind::LoadU16Dyn(..)
                | InstKind::ICmp(..)
        )
}

/// The block opens with a release of `h` behind nothing that names it:
/// an exit head the hoist built.
fn releases_first(insts: &[Inst], release: &InstKind, h: ValueId) -> bool {
    insts
        .iter()
        .find(|i| takes(&i.kind, h))
        .is_some_and(|i| i.kind == *release)
}

/// `old = load slot; str_drop(old); <retain here>; store h, slot`
/// at `insts[i - 2..=i]` of `b` becomes the swap behind `old != h`;
/// answers the two blocks it adds.
fn swap_unless_same(
    func: &mut Function,
    b: BlockId,
    i: usize,
    h: ValueId,
    str_drop: FuncId,
) -> Option<[BlockId; 2]> {
    let insts = &func.blocks[b.0 as usize].insts;
    if i < 2 {
        return None;
    }
    let (InstKind::Call(f, args), Some(old)) = (&insts[i - 1].kind, insts[i - 2].result) else {
        return None;
    };
    if *f != str_drop || args.as_slice() != [Operand::Value(old)] {
        return None;
    }
    let hv = Operand::Value(h);
    let same_slot = match (&insts[i - 2].kind, &insts[i].kind) {
        (InstKind::LoadDyn(_, p, o), InstKind::StoreDyn(v, q, r)) => *v == hv && p == q && o == r,
        (InstKind::LoadDynScaled8(_, p, o), InstKind::StoreDynScaled8(v, q, r)) => {
            *v == hv && p == q && o == r
        }
        (InstKind::Load(_, p, o), InstKind::Store(v, q, r)) => *v == hv && p == q && o == r,
        _ => false,
    };
    if !same_slot {
        return None;
    }
    let tail = new_block(func);
    let swap = new_block(func);
    let src = &mut func.blocks[b.0 as usize];
    let rest = src.insts.split_off(i);
    let drop_old = src.insts.pop().expect("the release of the old cell");
    let eq = push_value(&mut func.values, Type::Bool);
    func.blocks[b.0 as usize].insts.push(Inst {
        result: Some(eq),
        kind: InstKind::ICmp(IPred::Eq, Operand::Value(old), hv),
        origin: None,
    });
    let term = std::mem::replace(
        &mut func.blocks[b.0 as usize].term,
        Terminator::CondBr {
            cond: Operand::Value(eq),
            then_blk: tail,
            else_blk: swap,
        },
    );
    func.blocks[swap.0 as usize].insts = vec![drop_old, inst(InstKind::RcInc(hv))];
    func.blocks[swap.0 as usize].term = Terminator::Br(tail);
    func.blocks[tail.0 as usize].insts = rest;
    func.blocks[tail.0 as usize].term = term;
    Some([tail, swap])
}

fn inst(kind: InstKind) -> Inst {
    Inst {
        result: None,
        kind,
        origin: None,
    }
}
