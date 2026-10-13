//! The masked group path with its lanes packed two to a vector
//! register. Every lane runs the same instruction sequence under its
//! own flag, so lane `2q` and lane `2q+1`'s copies of one instruction
//! are one two-lane instruction, and the flags are lane masks: the
//! superword packing of unroll-and-jam's isomorphic lane copies
//! (Larsen & Amarasinghe 2000, as LLVM's SLP vectorizer picks them up
//! after `LoopUnrollAndJam`), in the masked form the loop vectorizer
//! gives a loop with early exits. `masked` keeps the block structure;
//! this module fills `GB`'s packing, `FB`, `R`'s unpacking and `N`.
//!
//! ```text
//! GB:  pre for every lane (scalar); pair q packs lanes 2q, 2q+1 into
//!      one vector per slot value and per snapshot; act_q = all ones
//! FB:  pair q: the inner step on vectors, cells written in place;
//!      go_q = act_q & the mask of each test the step passes (& !mask
//!      where passing means false); snap_q = go_q ? cells_q : snap_q;
//!      act_q = go_q ; cond_br lane 0 of act_0 & any other lane, FB, R
//! R:   slot 0 = lane 0 of pair 0, unpacked; then as `retire`
//! N:   every vector takes the next lane along (`ext`), the last lane
//!      idle; live -= 1
//! ```
//!
//! Why the result cannot differ: a lane of a vector holds exactly what
//! the lane's scalar slot held. The lane-wise operations are the
//! scalar ones (same rounding, nothing fused), a compare's mask is its
//! truth value, the select by mask is the scalar select, and the
//! exits fold into `go` as the scalar form folds them, so `masked`'s
//! argument carries over. A kept cell the step bumps by a constant
//! after its last exit test, through a sum nothing else reads, freezes
//! itself (`c = c + (go & k)`) instead of keeping a snapshot: once a
//! lane's `go` is false, the cell holds what the step began with,
//! which is what the snapshot kept, and that lane never steps again.
//! A slot value only the code after the inner loop reads stays one
//! scalar per lane, shifted as `masked` shifts it.

mod fit;
mod slots;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, BlockId, FPred, Function, IPred, InstKind, Operand, Terminator, Type, ValueId, VecInst,
    VecOp, VecShape, visit_value_operands,
};

use super::Plan;
use super::clone::{new_value, push};
use super::masked::{copy_of, shift as shift_scalars, slot_values};
use super::plan::term_value;
use super::retire::snapped;
pub(super) use fit::{Packing, packing};
use fit::{args, bump_by};
use slots::{group_test, shift, unpack_slot0};

type Lane = HashMap<ValueId, ValueId>;

const I64X2: Type = Type::Vec(VecShape::I64x2);
const F64X2: Type = Type::Vec(VecShape::F64x2);

fn shape_of(ty: Type) -> VecShape {
    match ty {
        Type::F64 => VecShape::F64x2,
        _ => VecShape::I64x2,
    }
}

/// A new vector cell in `b` holding `v`.
fn vcell(func: &mut Function, b: BlockId, ty: Type, v: ValueId) -> ValueId {
    let cell = new_value(func, ty);
    push(func, b, cell, InstKind::Copy(ty, Operand::Value(v)));
    cell
}

fn vtemp(func: &mut Function, b: BlockId, ty: Type, vi: VecInst) -> ValueId {
    let t = new_value(func, ty);
    push(func, b, t, InstKind::Vec(vi));
    t
}

/// One pair's vectors: its packed slot values and snapshots by scalar
/// id, and its flag.
struct Pair {
    slots: HashMap<ValueId, ValueId>,
    snaps: HashMap<ValueId, ValueId>,
    act: ValueId,
}

/// What every part of the packed group reads alike.
struct Group<'a> {
    p: &'a Plan,
    pk: &'a Packing,
    /// Lane 0's renaming at the group's start, its scalar slots
    /// included.
    lane0: &'a Lane,
    /// `GB`'s last block, where the splats go.
    cur: BlockId,
}

/// Everything after `GB`'s pre lanes, which end in `cur`; `g` is the
/// group header.
pub(super) fn steps(
    func: &mut Function,
    p: &Plan,
    cells: &HashSet<ValueId>,
    read_after_pre: &HashSet<ValueId>,
    g: BlockId,
    cur: BlockId,
    mut lanes: Vec<Lane>,
) {
    let pk = p.packing.as_ref().expect("a packed group has its packing");
    assert!(
        p.lanes == pk.lanes && p.lanes.is_multiple_of(2),
        "a packed group runs its planned pairs"
    );
    let slots = slot_values(p, cells, read_after_pre, &lanes[0]);
    let (packed, scalar): (Vec<ValueId>, Vec<ValueId>) =
        slots.iter().partition(|v| pk.packed.contains(v));
    for lane in lanes.iter_mut() {
        // a lane cell is its own slot
        for &v in scalar.iter().filter(|v| !cells.contains(v)) {
            let slot = copy_of(func, cur, lane.get(&v).copied().unwrap_or(v));
            lane.insert(v, slot);
        }
    }
    let kept = snapped(func, p);
    let ones = vtemp(
        func,
        cur,
        I64X2,
        VecInst::Splat(VecShape::I64x2, Operand::ConstI64(-1)),
    );
    let mut splatted: HashSet<ValueId> = HashSet::default();
    let mut pairs: Vec<Pair> = Vec::with_capacity(p.lanes / 2);
    for q in 0..p.lanes / 2 {
        let mut pair = Pair {
            slots: HashMap::default(),
            snaps: HashMap::default(),
            act: vcell(func, cur, I64X2, ones),
        };
        for &v in &packed {
            let shape = shape_of(func.value_type(v));
            let lo = lanes[2 * q].get(&v).copied().unwrap_or(v);
            let hi = lanes[2 * q + 1].get(&v).copied().unwrap_or(v);
            let both = match pk.starts.get(&v) {
                Some(&start) => {
                    splatted.extend([lo, hi]);
                    VecInst::Splat(shape, start)
                }
                None => VecInst::Pair(shape, Operand::Value(lo), Operand::Value(hi)),
            };
            let t = vtemp(func, cur, Type::Vec(shape), both);
            let cell = vcell(func, cur, Type::Vec(shape), t);
            pair.slots.insert(v, cell);
        }
        for &c in kept.iter().filter(|c| !pk.is_frozen(**c)) {
            let ty = func.value_type(pair.slots[&c]);
            let snap = vcell(func, cur, ty, pair.slots[&c]);
            pair.snaps.insert(c, snap);
        }
        pairs.push(pair);
    }
    // a lane's copy only its pair's splat stood for
    drop_unread(func, g, &splatted);
    let live = new_value(func, Type::I64);
    let lanes_left = Operand::ConstI64(p.lanes as i64);
    push(func, cur, live, InstKind::Copy(Type::I64, lanes_left));
    let fb = func.add_block();
    let r = func.add_block();
    let n = func.add_block();
    let ft = func.add_block();
    func.blocks[cur.0 as usize].term = Terminator::Br(fb);
    let group = Group {
        p,
        pk,
        lane0: &lanes[0],
        cur,
    };
    let mut splats: HashMap<Operand, ValueId> = HashMap::default();
    for pair in &mut pairs {
        pair_step(func, &group, pair, fb, &mut splats);
    }
    group_test(func, &pairs, fb, fb, r);
    group_test(func, &pairs, ft, fb, r);
    unpack_slot0(func, &group, cells, read_after_pre, &pairs[0], (r, n));
    shift(func, &pairs, n);
    shift_scalars(func, n, &scalar, &lanes);
    let dec = InstKind::BinOp(BinOp::Sub, Operand::Value(live), Operand::ConstI64(1));
    let left = new_value(func, Type::I64);
    push(func, n, left, dec);
    push(
        func,
        n,
        live,
        InstKind::Copy(Type::I64, Operand::Value(left)),
    );
    let more = new_value(func, Type::Bool);
    let test = InstKind::ICmp(IPred::Sgt, Operand::Value(live), Operand::ConstI64(0));
    push(func, n, more, test);
    func.blocks[n.0 as usize].term = Terminator::CondBr {
        cond: Operand::Value(more),
        then_blk: ft,
        else_blk: g,
    };
}

/// Drops the definitions of `values` nothing reads, all of them in
/// `GB`'s blocks: `g` and every block made after it.
fn drop_unread(func: &mut Function, g: BlockId, values: &HashSet<ValueId>) {
    let mut read: HashSet<ValueId> = HashSet::default();
    for block in &func.blocks[g.0 as usize..] {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                if values.contains(&v) {
                    read.insert(v);
                }
            });
        }
        read.extend(term_value(&block.term).filter(|v| values.contains(v)));
    }
    for block in &mut func.blocks[g.0 as usize..] {
        block.insts.retain(|i| {
            !i.result
                .is_some_and(|r| values.contains(&r) && !read.contains(&r))
        });
    }
}

/// One pair's inner step in `fb`: the step's instructions on vectors,
/// the exit tests folded into `go`, then the snapshots and the flag.
/// A loop-invariant operand is splat once, in `cur`.
fn pair_step(
    func: &mut Function,
    group: &Group,
    pair: &mut Pair,
    fb: BlockId,
    splats: &mut HashMap<Operand, ValueId>,
) {
    let mut vmap: HashMap<ValueId, ValueId> = pair.slots.clone();
    let mut go = pair.act;
    let inner = group.p.inner_order();
    let v = Operand::Value;
    for &b in &inner {
        let block = func.blocks[b.0 as usize].clone();
        for inst in &block.insts {
            let r = inst.result.expect("planned regions define values");
            let mut ops = args(&inst.kind).into_iter();
            let mut arg = |func: &mut Function| {
                let op = ops.next().expect("a packed instruction reads a value");
                v(vector_of(func, group, op, &vmap, splats))
            };
            let (ty, vi) = match &inst.kind {
                InstKind::BinOp(BinOp::Add, ..) if group.pk.bump_of(r).is_some() => {
                    let (c, k) = bump_by(&inst.kind).expect("a frozen bump is a sum");
                    let cell = v(vmap[&c]);
                    if k == 1 {
                        (I64X2, VecInst::BinOp(VecOp::Sub, cell, v(go)))
                    } else {
                        let k = Operand::ConstI64(k);
                        let step = v(vector_of(func, group, k, &vmap, splats));
                        let masked =
                            vtemp(func, fb, I64X2, VecInst::BinOp(VecOp::And, v(go), step));
                        (I64X2, VecInst::BinOp(VecOp::Add, cell, v(masked)))
                    }
                }
                InstKind::BinOp(op, _, _) => {
                    let (ty, vop) = match op {
                        BinOp::FAdd => (F64X2, VecOp::FAdd),
                        BinOp::FSub => (F64X2, VecOp::FSub),
                        BinOp::FMul => (F64X2, VecOp::FMul),
                        BinOp::Add => (I64X2, VecOp::Add),
                        _ => (I64X2, VecOp::Sub),
                    };
                    let (a, b) = (arg(func), arg(func));
                    (ty, VecInst::BinOp(vop, a, b))
                }
                InstKind::FCmp(pred, _, _) => {
                    let (a, b) = (arg(func), arg(func));
                    let (vop, a, b) = match pred {
                        FPred::Oeq => (VecOp::FCmEq, a, b),
                        FPred::Ogt => (VecOp::FCmGt, a, b),
                        FPred::Oge => (VecOp::FCmGe, a, b),
                        FPred::Olt => (VecOp::FCmGt, b, a),
                        _ => (VecOp::FCmGe, b, a),
                    };
                    (I64X2, VecInst::BinOp(vop, a, b))
                }
                InstKind::ICmp(pred, _, _) => {
                    let (a, b) = (arg(func), arg(func));
                    let (vop, a, b) = match pred {
                        IPred::Eq => (VecOp::CmEq, a, b),
                        IPred::Sgt => (VecOp::CmGt, a, b),
                        IPred::Sge => (VecOp::CmGe, a, b),
                        IPred::Slt => (VecOp::CmGt, b, a),
                        _ => (VecOp::CmGe, b, a),
                    };
                    (I64X2, VecInst::BinOp(vop, a, b))
                }
                InstKind::Copy(ty, _) => {
                    let ty = Type::Vec(shape_of(*ty));
                    let src = arg(func);
                    let cell = *vmap.entry(r).or_insert_with(|| new_value(func, ty));
                    push(func, fb, cell, InstKind::Copy(ty, src));
                    continue;
                }
                other => unreachable!("packing an unpacked instruction {other:?}"),
            };
            let out = vtemp(func, fb, ty, vi);
            vmap.insert(r, out);
        }
        // `fit` saw every branch of the step test a packed compare
        if let Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            ..
        } = block.term
        {
            let mask = v(vmap[&c]);
            let op = if inner.contains(&then_blk) {
                VecOp::And
            } else {
                VecOp::AndNot
            };
            go = vtemp(func, fb, I64X2, VecInst::BinOp(op, v(go), mask));
        }
    }
    let mut snaps: Vec<(ValueId, ValueId)> = pair.snaps.iter().map(|(c, s)| (*c, *s)).collect();
    snaps.sort_by_key(|(c, _)| c.0);
    for (c, s) in snaps {
        let ty = func.value_type(s);
        let kept = vtemp(func, fb, ty, VecInst::Select(v(go), v(vmap[&c]), v(s)));
        push(func, fb, s, InstKind::Copy(ty, v(kept)));
    }
    push(func, fb, pair.act, InstKind::Copy(I64X2, v(go)));
}

/// The vector holding `op` for one pair: its packed value, or a splat
/// of the loop-invariant scalar (lane 0's copy of a shared value),
/// made once in `cur`.
fn vector_of(
    func: &mut Function,
    group: &Group,
    op: Operand,
    vmap: &HashMap<ValueId, ValueId>,
    splats: &mut HashMap<Operand, ValueId>,
) -> ValueId {
    if let Operand::Value(x) = op
        && let Some(&vec) = vmap.get(&x)
    {
        return vec;
    }
    if let Some(&s) = splats.get(&op) {
        return s;
    }
    let (shape, scalar) = match op {
        Operand::Value(x) => (
            shape_of(func.value_type(x)),
            Operand::Value(group.lane0.get(&x).copied().unwrap_or(x)),
        ),
        Operand::ConstF64(_) => (VecShape::F64x2, op),
        _ => (VecShape::I64x2, op),
    };
    let splat = VecInst::Splat(shape, scalar);
    let s = vtemp(func, group.cur, Type::Vec(shape), splat);
    splats.insert(op, s);
    s
}
