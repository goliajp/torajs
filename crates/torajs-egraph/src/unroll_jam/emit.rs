//! Builds the jammed group path beside the original loop:
//!
//! ```text
//! PH:  bk = bound - (LANES-1)·C ; no-wrap check     (loop entry)
//! G:   cond_br iv <pred> bk, GB, H                  (group header)
//! GB:  lane j: i_j = iv + j·C ; pre(i_j) on renamed cells; a pre guard
//!      leaving the loop → H instead
//! F0…: lane j's inner header test; any lane done → R0
//! FB:  every lane's inner body (its guards → H too) ; br F0
//! Rj:  original cells = lane j's ; br Tj
//! Tj:  clone of inner loop + post + latch; latch → R(j+1), last → G
//! ```
//!
//! The original loop stays as it was and runs whatever a group cannot.

use std::collections::HashMap;

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Operand, Terminator, Type, ValueId,
};

use super::clone::{
    cell_set, clone_into, clone_value, new_value, push, reads_outside, retarget, tail_clone,
};
use super::{LANES, Plan};

pub(super) fn apply(func: &mut Function, p: &Plan) {
    let h = p.header;
    let cells = cell_set(func);
    let read_after_pre = reads_outside(func, &p.pre_order());
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
    let heads: Vec<BlockId> = (0..LANES).map(|_| func.add_block()).collect();
    let fb = func.add_block();
    let resumes: Vec<BlockId> = (0..LANES).map(|_| func.add_block()).collect();
    let (mut lanes, cur) = pre_lanes(func, p, gb);
    func.blocks[cur.0 as usize].term = Terminator::Br(heads[0]);
    inner_heads(func, p, &heads, fb, resumes[0], &mut lanes);
    let last = fused_body(func, p, fb, &mut lanes);
    func.blocks[last.0 as usize].term = Terminator::Br(heads[0]);
    for j in 0..LANES {
        let next = if j + 1 < LANES { resumes[j + 1] } else { g };
        let entry = tail_clone(func, p, &cells, &lanes[j], next);
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

/// `GB`: every lane's `pre`, lane by lane from `gb`. Returns each
/// lane's renaming and the block the last lane ends in.
fn pre_lanes(
    func: &mut Function,
    p: &Plan,
    gb: BlockId,
) -> (Vec<HashMap<ValueId, ValueId>>, BlockId) {
    let mut lanes: Vec<HashMap<ValueId, ValueId>> = Vec::with_capacity(LANES);
    let mut cur = gb;
    for j in 0..LANES {
        let mut map: HashMap<ValueId, ValueId> = HashMap::new();
        if j > 0 {
            let ij = new_value(func, Type::I64);
            let off = p.step * j as i64;
            let add = InstKind::BinOp(BinOp::Add, Operand::Value(p.iv), Operand::ConstI64(off));
            push(func, cur, ij, add);
            map.insert(p.iv, ij);
        }
        for &c in &p.lane_cells {
            let fresh = clone_value(func, c);
            map.insert(c, fresh);
        }
        for b in p.pre_order() {
            let insts = func.blocks[b.0 as usize].insts.clone();
            for inst in insts.iter().filter(|i| i.result != p.skip) {
                clone_into(func, cur, inst, &mut map);
            }
            cur = lane_guard(func, p, b, cur, &map);
        }
        lanes.push(map);
    }
    (lanes, cur)
}

/// `F0…`: lane j's inner header test in `heads[j]`; the last lane goes
/// on to `fb`, any lane done to `done`.
fn inner_heads(
    func: &mut Function,
    p: &Plan,
    heads: &[BlockId],
    fb: BlockId,
    done: BlockId,
    lanes: &mut [HashMap<ValueId, ValueId>],
) {
    let ih = p.inner_header;
    let ih_term = func.blocks[ih.0 as usize].term.clone();
    let Terminator::CondBr {
        cond: Operand::Value(ic),
        ..
    } = ih_term
    else {
        unreachable!("planned inner header ends in cond_br");
    };
    for (j, map) in lanes.iter_mut().enumerate() {
        let insts = func.blocks[ih.0 as usize].insts.clone();
        for inst in &insts {
            clone_into(func, heads[j], inst, map);
        }
        let stay = if j + 1 < LANES { heads[j + 1] } else { fb };
        let (then_blk, else_blk) = if p.inner_on_true {
            (stay, done)
        } else {
            (done, stay)
        };
        func.blocks[heads[j].0 as usize].term = Terminator::CondBr {
            cond: Operand::Value(map[&ic]),
            then_blk,
            else_blk,
        };
    }
}

/// `FB`: every lane's inner body from `fb`. Returns the block it ends
/// in.
fn fused_body(
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

/// A pre or inner-body guard leaving the loop, cloned for one lane: the
/// group falls back to the original header (nothing is written yet)
/// when it fires. Returns the block the lane continues in.
fn lane_guard(
    func: &mut Function,
    p: &Plan,
    b: BlockId,
    cur: BlockId,
    map: &HashMap<ValueId, ValueId>,
) -> BlockId {
    let Terminator::CondBr {
        cond: Operand::Value(c),
        then_blk,
        ..
    } = func.blocks[b.0 as usize].term
    else {
        return cur;
    };
    if b == p.header {
        return cur;
    }
    let next = func.add_block();
    let cond = Operand::Value(map.get(&c).copied().unwrap_or(c));
    let (then_blk, else_blk) = if p.body.contains(&then_blk) {
        (next, p.header)
    } else {
        (p.header, next)
    };
    func.blocks[cur.0 as usize].term = Terminator::CondBr {
        cond,
        then_blk,
        else_blk,
    };
    next
}

/// `PH`: the group bound `bk`, and whether computing it wrapped. A
/// constant bound was checked when planning.
fn entry_block(func: &mut Function, p: &Plan, ph: BlockId, g: BlockId) -> Operand {
    let span = p.step * (LANES as i64 - 1);
    match p.bound {
        Operand::ConstI64(n) => {
            func.blocks[ph.0 as usize].term = Terminator::Br(g);
            Operand::ConstI64(n - span)
        }
        bound => {
            let bk = new_value(func, Type::I64);
            push(
                func,
                ph,
                bk,
                InstKind::BinOp(BinOp::Sub, bound, Operand::ConstI64(span)),
            );
            let ok = new_value(func, Type::Bool);
            push(
                func,
                ph,
                ok,
                InstKind::ICmp(IPred::Slt, Operand::Value(bk), bound),
            );
            func.blocks[ph.0 as usize].term = Terminator::CondBr {
                cond: Operand::Value(ok),
                then_blk: g,
                else_blk: p.header,
            };
            Operand::Value(bk)
        }
    }
}
