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

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
    rewrite_value_operands, visit_value_operands,
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
    func.blocks[cur.0 as usize].term = Terminator::Br(heads[0]);
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
            (stay, resumes[0])
        } else {
            (resumes[0], stay)
        };
        func.blocks[heads[j].0 as usize].term = Terminator::CondBr {
            cond: Operand::Value(map[&ic]),
            then_blk,
            else_blk,
        };
    }
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
    func.blocks[cur.0 as usize].term = Terminator::Br(heads[0]);
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

/// One lane finishing alone: the inner loop, post and latch cloned on
/// the original cells, the latch's back edge sent to `next`. Values
/// `pre` computed come from this lane's copies; every other non-cell
/// def gets a fresh id. Returns the clone's entry (inner header).
fn tail_clone(
    func: &mut Function,
    p: &Plan,
    cells: &HashSet<ValueId>,
    lane: &HashMap<ValueId, ValueId>,
    next: BlockId,
) -> BlockId {
    let blocks: Vec<BlockId> = p
        .inner_order()
        .into_iter()
        .chain(p.post.iter().copied())
        .collect();
    let mut map: HashMap<ValueId, ValueId> = lane
        .iter()
        // only what `pre` computed: every cell, the inner loop's
        // temporaries included, is read and written under its own id
        .filter(|(v, _)| **v != p.iv && !cells.contains(v) && !p.escaping.contains(v))
        .map(|(v, n)| (*v, *n))
        .collect();
    let bmap: HashMap<BlockId, BlockId> = blocks.iter().map(|b| (*b, func.add_block())).collect();
    for &b in &blocks {
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
    for &b in &blocks {
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
        retarget(
            &mut term,
            |t| bmap.get(&t).copied().unwrap_or(t),
            p.header,
            Some(next),
        );
        func.blocks[nb.0 as usize].term = term;
    }
    bmap[&p.inner_header]
}

/// Values every def of which is a `Copy`: the cells clones keep under
/// their own id.
fn cell_set(func: &Function) -> HashSet<ValueId> {
    let mut copy: HashSet<ValueId> = HashSet::new();
    let mut other: HashSet<ValueId> = HashSet::new();
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
fn reads_outside(func: &Function, blocks: &[BlockId]) -> HashSet<ValueId> {
    let mut out = HashSet::new();
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
fn retarget(
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
fn clone_into(func: &mut Function, dst: BlockId, inst: &Inst, map: &mut HashMap<ValueId, ValueId>) {
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

fn clone_value(func: &mut Function, like: ValueId) -> ValueId {
    let ty = func.values[like.0 as usize].ty;
    new_value(func, ty)
}

fn new_value(func: &mut Function, ty: Type) -> ValueId {
    let id = ValueId(func.values.len() as u32);
    func.values.push(ValueInfo { ty, name: None });
    id
}

fn push(func: &mut Function, b: BlockId, result: ValueId, kind: InstKind) {
    func.blocks[b.0 as usize].insts.push(Inst {
        result: Some(result),
        kind,
        origin: None,
    });
}
