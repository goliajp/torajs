//! Builds the jammed group path beside the original loop:
//!
//! ```text
//! PH:  bk = bound - (L-1)·C ; no-wrap check        (loop entry)
//! G:   cond_br iv <pred> bk, GB, H                  (group header)
//! GB:  lane j: i_j = iv + j·C ; pre(i_j) on renamed cells; a pre guard
//!      leaving the loop → H instead
//! F0…: lane j's inner header test; any lane done → R0
//! FB:  every lane's inner body (its guards → H too) ; br F0
//! Rj:  original cells = lane j's ; br Tj
//! Tj:  clone of inner loop + post + latch; latch → R(j+1), last → G
//! ```
//!
//! An inner loop with an early exit steps its lanes under masks
//! instead (`masked`).
//!
//! The original loop stays as it was and runs whatever a group cannot.

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    BinOp, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use super::Plan;
use super::clone::{
    cell_set, clone_into, clone_value, new_value, push, reads_outside, retarget, tail_clone,
};
use super::guards::{entry_block, guard_shared, lane_guard};
use super::heads::inner_heads;

pub(super) fn apply(func: &mut Function, p: &Plan) {
    let cells = cell_set(func);
    let computed;
    let read_after_pre = match &p.read_after_pre {
        Some(reads) => reads,
        None => {
            computed = reads_outside(func, &p.pre_order());
            &computed
        }
    };
    let (g, gb) = group_entry(func, p);
    if p.masked {
        let (lanes, cur) = pre_lanes(func, p, gb);
        super::masked::steps(func, p, &cells, read_after_pre, g, cur, lanes);
        return;
    }
    let heads: Vec<BlockId> = (0..p.lanes).map(|_| func.add_block()).collect();
    let fb = func.add_block();
    let resumes: Vec<BlockId> = (0..p.lanes).map(|_| func.add_block()).collect();
    let (mut lanes, cur) = pre_lanes(func, p, gb);
    func.blocks[cur.0 as usize].term = Terminator::Br(heads[0]);
    inner_heads(func, p, &heads, fb, resumes[0], &mut lanes);
    let last = fused_body(func, p, fb, &mut lanes);
    func.blocks[last.0 as usize].term = Terminator::Br(heads[0]);
    for j in 0..p.lanes {
        let next = if j + 1 < p.lanes { resumes[j + 1] } else { g };
        let entry = tail_clone(func, p, &cells, &lanes[j], next)[&p.inner_header];
        let r = resumes[j];
        for &c in p.lane_cells.iter().filter(|c| read_after_pre.contains(c)) {
            let ty = func.values[c.0 as usize].ty;
            push(func, r, c, InstKind::Copy(ty, Operand::Value(lanes[j][&c])));
        }
        if let Some(&ij) = lanes[j].get(&p.iv) {
            push(func, r, p.iv, InstKind::Copy(Type::I64, Operand::Value(ij)));
        }
        func.blocks[r.0 as usize].term = Terminator::Br(entry);
    }
}

/// `PH` in front of the loop and `G` testing whether a whole group
/// remains; returns `G` and the group's first block `GB`.
fn group_entry(func: &mut Function, p: &Plan) -> (BlockId, BlockId) {
    let h = p.header;
    let ph = func.add_block();
    for b in 0..ph.0 as usize {
        if !p.body.contains(&BlockId(b as u32)) {
            retarget(&mut func.blocks[b].term, |t| t, h, Some(ph));
        }
    }
    let g = func.add_block();
    let gb = func.add_block();
    let bk = entry_block(func, p, ph, g);
    let gv = new_value(func, Type::Bool);
    push(
        func,
        g,
        gv,
        InstKind::ICmp(p.pred, Operand::Value(p.iv), bk),
    );
    func.blocks[g.0 as usize].term = Terminator::CondBr {
        cond: Operand::Value(gv),
        then_blk: gb,
        else_blk: h,
    };
    (g, gb)
}

/// `GB`: every lane's `pre`, lane by lane from `gb`. Returns each
/// lane's renaming and the block the last lane ends in.
pub(super) fn pre_lanes(
    func: &mut Function,
    p: &Plan,
    gb: BlockId,
) -> (Vec<HashMap<ValueId, ValueId>>, BlockId) {
    let mut lanes: Vec<HashMap<ValueId, ValueId>> = Vec::with_capacity(p.lanes);
    let mut cur = gb;
    for j in 0..p.lanes {
        let mut map: HashMap<ValueId, ValueId> = HashMap::default();
        if j > 0 {
            let ij = new_value(func, Type::I64);
            let off = p.step * j as i64;
            let add = InstKind::BinOp(BinOp::Add, Operand::Value(p.iv), Operand::ConstI64(off));
            push(func, cur, ij, add);
            map.insert(p.iv, ij);
        }
        for &c in &p.lane_cells {
            let mine = match lanes.first() {
                Some(lane0) if p.shared.contains(&c) => lane0[&c],
                _ => clone_value(func, c),
            };
            map.insert(c, mine);
        }
        for b in p.pre_order() {
            let insts = func.blocks[b.0 as usize].insts.clone();
            for inst in insts.iter().filter(|i| i.result != p.skip) {
                if !reuse_shared(p, inst, lanes.first(), &mut map) {
                    clone_into(func, cur, inst, &mut map);
                }
            }
            if lanes.is_empty() || !guard_shared(func, p, b) {
                cur = lane_guard(func, p, b, cur, &map);
            }
        }
        lanes.push(map);
    }
    (lanes, cur)
}

/// The fused body when the lanes share nothing: each lane's copy of
/// the chain after the one before it.
fn lane_after_lane(
    func: &mut Function,
    p: &Plan,
    fb: BlockId,
    lanes: &mut [HashMap<ValueId, ValueId>],
) -> BlockId {
    let mut cur = fb;
    for map in lanes.iter_mut() {
        for &b in &p.chain {
            let insts = func.blocks[b.0 as usize].insts.clone();
            for inst in &insts {
                clone_into(func, cur, inst, map);
            }
            cur = lane_guard(func, p, b, cur, map);
        }
    }
    cur
}

/// A shared instruction, cloned once for the first lane and read by
/// the others.
fn clone_shared(
    func: &mut Function,
    p: &Plan,
    cur: BlockId,
    inst: &Inst,
    lanes: &mut [HashMap<ValueId, ValueId>],
) {
    let (first, rest) = lanes.split_at_mut(1);
    clone_into(func, cur, inst, &mut first[0]);
    for map in rest.iter_mut() {
        reuse_shared(p, inst, Some(&first[0]), map);
    }
}

/// `FB`: every lane's inner body from `fb`. Returns the block it ends
/// in. With values the lanes share, each run of a block up to a shared
/// instruction goes lane by lane, then the shared one once: every lane
/// still runs its own instructions in order (a compare stays next to
/// the select reading it), and a shared value is defined before any
/// lane reads it and updated after every lane has read the old one.
/// A lane's guard follows its copy of the block's last run, next to
/// the compare it tests, or all lanes' copies when that run ends in a
/// shared instruction (a guard only sends the group back to the
/// original loop, whichever fires first); a shared guard is tested
/// once.
fn fused_body(
    func: &mut Function,
    p: &Plan,
    fb: BlockId,
    lanes: &mut [HashMap<ValueId, ValueId>],
) -> BlockId {
    if p.shared.is_empty() {
        return lane_after_lane(func, p, fb, lanes);
    }
    let mut cur = fb;
    for &b in &p.chain {
        let insts = func.blocks[b.0 as usize].insts.clone();
        let is_shared = |i: &Inst| i.result.is_some_and(|r| p.shared.contains(&r));
        let one_guard = guard_shared(func, p, b);
        // each lane's guard right after its copy of the last run, unless
        // a shared instruction closes the block or the guard is shared
        let per_lane = !one_guard && insts.last().is_none_or(|i| !is_shared(i));
        let runs: Vec<&[Inst]> = insts.split_inclusive(is_shared).collect();
        let last = runs.len().saturating_sub(1);
        for (k, run) in runs.iter().enumerate() {
            let (own, shared) = match run.split_last() {
                Some((tail, own)) if is_shared(tail) => (own, Some(tail)),
                _ => (*run, None),
            };
            for map in lanes.iter_mut() {
                for inst in own {
                    clone_into(func, cur, inst, map);
                }
                if per_lane && k == last {
                    cur = lane_guard(func, p, b, cur, map);
                }
            }
            if let Some(inst) = shared {
                clone_shared(func, p, cur, inst, lanes);
            }
        }
        if runs.is_empty() && per_lane {
            for map in lanes.iter() {
                cur = lane_guard(func, p, b, cur, map);
            }
        } else if !per_lane {
            let guarded = if one_guard { 1 } else { p.lanes };
            for map in lanes.iter().take(guarded) {
                cur = lane_guard(func, p, b, cur, map);
            }
        }
    }
    cur
}

/// For a lane past the first: a shared `inst` is lane 0's, so the
/// lane names lane 0's value and emits nothing.
pub(super) fn reuse_shared(
    p: &Plan,
    inst: &Inst,
    lane0: Option<&HashMap<ValueId, ValueId>>,
    map: &mut HashMap<ValueId, ValueId>,
) -> bool {
    match (inst.result, lane0) {
        (Some(r), Some(lane0)) if p.shared.contains(&r) => {
            map.insert(r, lane0[&r]);
            true
        }
        _ => false,
    }
}
