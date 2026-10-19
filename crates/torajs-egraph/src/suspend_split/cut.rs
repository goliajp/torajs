//! Cut the body at its suspension points and find what lives across
//! each one.
//!
//! Every `Suspend` ends its block: the instructions after it move to
//! a fresh landing block, joined to the cut block by a branch for as
//! long as the analysis needs the path the suspension stands for. A
//! promise suspension's result is defined at the top of its landing,
//! as the read of the frame word the resumption left the settled cell
//! in. The block-level live-in set of a landing is then exactly the
//! values that cross that suspension point.

use torajs_core::ssa::liveness_blocks::compute_block_liveness;
use torajs_core::ssa::suspend::FRAME_AWAITING_OFF;
use torajs_core::ssa::{
    Block, BlockId, Function, Inst, InstKind, Operand, SuspendLane, Terminator, ValueId,
};

/// One suspension point, numbered from 1 in the order it was found.
pub(super) struct Point {
    /// Position of the cut block; its last instruction is the
    /// `Suspend`, and it branches to the landing until the park
    /// replaces both.
    pub(super) cut: usize,
    pub(super) landing: BlockId,
    pub(super) lane: SuspendLane,
    /// The values that cross this point.
    pub(super) cross: Vec<ValueId>,
}

/// A new empty block at the end. Blocks are numbered by position here,
/// as every pass after this one reads them.
pub(super) fn fresh_block(f: &mut Function) -> BlockId {
    let id = BlockId(f.blocks.len() as u32);
    f.blocks.push(Block {
        id,
        insts: Vec::new(),
        term: Terminator::Unreachable,
    });
    id
}

pub(super) fn position(_f: &Function, id: BlockId) -> usize {
    id.0 as usize
}

/// Cut at every suspension and collect the crossing sets. A block is
/// cut once, at all its suspensions together: the pieces after the
/// first go to landings in order, the last taking the block's
/// terminator.
pub(super) fn cut_points(f: &mut Function, frame: ValueId) -> Vec<Point> {
    assert!(
        (f.blocks.iter().enumerate()).all(|(i, b)| b.id.0 as usize == i),
        "internal error: `{}` reaches the split with its blocks out of order",
        f.name
    );
    let mut points: Vec<Point> = Vec::new();
    for at in 0..f.blocks.len() {
        if !(f.blocks[at].insts.iter()).any(|i| matches!(i.kind, InstKind::Suspend(..))) {
            continue;
        }
        let insts = std::mem::take(&mut f.blocks[at].insts);
        let term = std::mem::replace(&mut f.blocks[at].term, Terminator::Unreachable);
        let mut cur = at;
        let mut piece: Vec<Inst> = Vec::new();
        for mut inst in insts {
            let InstKind::Suspend(_, lane, ..) = inst.kind else {
                piece.push(inst);
                continue;
            };
            let landing = fresh_block(f);
            let result = inst.result.take();
            piece.push(inst);
            f.blocks[cur].insts = std::mem::take(&mut piece);
            f.blocks[cur].term = Terminator::Br(landing);
            if let Some(r) = result {
                let ty = f.values[r.0 as usize].ty;
                piece.push(load(r, ty, frame, FRAME_AWAITING_OFF));
                piece.push(store(Operand::ConstI64(0), frame, FRAME_AWAITING_OFF));
            }
            points.push(Point {
                cut: cur,
                landing,
                lane,
                cross: Vec::new(),
            });
            cur = position(f, landing);
        }
        f.blocks[cur].insts = piece;
        f.blocks[cur].term = term;
    }
    if points.is_empty() {
        return points;
    }
    let live = compute_block_liveness(f);
    for p in &mut points {
        let l = position(f, p.landing);
        p.cross = (live.live_in[l].iter())
            .map(ValueId)
            .filter(|&v| v != frame)
            .collect();
    }
    points
}

pub(super) fn load(r: ValueId, ty: torajs_core::ssa::Type, base: ValueId, off: u64) -> Inst {
    Inst {
        result: Some(r),
        kind: InstKind::Load(ty, Operand::Value(base), off),
        origin: None,
    }
}

pub(super) fn store(v: Operand, base: ValueId, off: u64) -> Inst {
    Inst {
        result: None,
        kind: InstKind::Store(v, Operand::Value(base), off),
        origin: None,
    }
}
