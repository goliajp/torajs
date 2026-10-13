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
//! after its last exit test freezes itself (`c = c + (go & k)`) instead
//! of keeping a snapshot: once a lane's `go` is false, the cell holds
//! what the step began with, which is what the snapshot kept, and that
//! lane never steps again.

mod fit;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, BlockId, FPred, Function, IPred, InstKind, Operand, Terminator, Type, ValueId, VecInst,
    VecOp, VecShape,
};

use super::Plan;
use super::clone::{new_value, push};
use super::masked::slot_values;
use super::retire::{Slot0, retire, snapped};
use fit::args;
pub(super) use fit::{Packing, lanes, packing};

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

/// One pair's vectors: its slot values and snapshots by scalar id,
/// and its flag.
struct Pair {
    slots: HashMap<ValueId, ValueId>,
    snaps: HashMap<ValueId, ValueId>,
    act: ValueId,
}

/// Everything after `GB`'s pre lanes, which end in `cur`; `g` is the
/// group header.
pub(super) fn steps(
    func: &mut Function,
    p: &Plan,
    pk: &Packing,
    cells: &HashSet<ValueId>,
    read_after_pre: &HashSet<ValueId>,
    g: BlockId,
    cur: BlockId,
    lanes: Vec<Lane>,
) {
    let slots = slot_values(p, cells, read_after_pre, &lanes[0]);
    let kept = snapped(func, p);
    let ones = vtemp(
        func,
        cur,
        I64X2,
        VecInst::Splat(VecShape::I64x2, Operand::ConstI64(-1)),
    );
    let mut pairs: Vec<Pair> = Vec::with_capacity(p.lanes / 2);
    for q in 0..p.lanes / 2 {
        let mut pair = Pair {
            slots: HashMap::default(),
            snaps: HashMap::default(),
            act: vcell(func, cur, I64X2, ones),
        };
        for &v in &slots {
            let ty = Type::Vec(shape_of(func.value_type(v)));
            let lo = lanes[2 * q].get(&v).copied().unwrap_or(v);
            let hi = lanes[2 * q + 1].get(&v).copied().unwrap_or(v);
            let packed = VecInst::Pair(
                shape_of(func.value_type(v)),
                Operand::Value(lo),
                Operand::Value(hi),
            );
            let t = vtemp(func, cur, ty, packed);
            pair.slots.insert(v, vcell(func, cur, ty, t));
        }
        for &c in kept.iter().filter(|c| !pk.frozen.contains(c)) {
            let ty = func.value_type(pair.slots[&c]);
            let snap = vcell(func, cur, ty, pair.slots[&c]);
            pair.snaps.insert(c, snap);
        }
        pairs.push(pair);
    }
    let live = new_value(func, Type::I64);
    let lanes_left = Operand::ConstI64(p.lanes as i64);
    push(func, cur, live, InstKind::Copy(Type::I64, lanes_left));
    let fb = func.add_block();
    let r = func.add_block();
    let n = func.add_block();
    let ft = func.add_block();
    func.blocks[cur.0 as usize].term = Terminator::Br(fb);
    let mut splats: HashMap<Operand, ValueId> = HashMap::default();
    for q in 0..pairs.len() {
        pair_step(func, p, pk, &lanes[0], &mut pairs[q], fb, cur, &mut splats);
    }
    group_test(func, &pairs, fb, fb, r);
    group_test(func, &pairs, ft, fb, r);
    unpack_slot0(
        func,
        p,
        pk,
        cells,
        read_after_pre,
        &lanes[0],
        &pairs[0],
        r,
        n,
    );
    shift(func, &pairs, n);
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

/// One pair's inner step in `fb`: the step's instructions on vectors,
/// the exit tests folded into `go`, then the snapshots and the flag.
/// A loop-invariant operand is splat once, in `cur`.
#[allow(clippy::too_many_arguments)]
fn pair_step(
    func: &mut Function,
    p: &Plan,
    pk: &Packing,
    lane0: &Lane,
    pair: &mut Pair,
    fb: BlockId,
    cur: BlockId,
    splats: &mut HashMap<Operand, ValueId>,
) {
    let mut vmap: HashMap<ValueId, ValueId> = pair.slots.clone();
    let mut go = pair.act;
    let inner = p.inner_order();
    let v = Operand::Value;
    for &b in &inner {
        let block = func.blocks[b.0 as usize].clone();
        for inst in &block.insts {
            let r = inst.result.expect("planned regions define values");
            let mut ops = args(&inst.kind).into_iter();
            let mut arg = |func: &mut Function| {
                let op = ops.next().expect("a packed instruction reads a value");
                v(vector_of(func, op, &vmap, lane0, cur, splats))
            };
            let (ty, vi) = match &inst.kind {
                InstKind::BinOp(BinOp::Add, Operand::Value(c), Operand::ConstI64(k))
                | InstKind::BinOp(BinOp::Add, Operand::ConstI64(k), Operand::Value(c))
                    if pk.frozen.contains(c) =>
                {
                    let cell = v(vmap[c]);
                    if *k == 1 {
                        (I64X2, VecInst::BinOp(VecOp::Sub, cell, v(go)))
                    } else {
                        let step = v(vector_of(
                            func,
                            Operand::ConstI64(*k),
                            &vmap,
                            lane0,
                            cur,
                            splats,
                        ));
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
    op: Operand,
    vmap: &HashMap<ValueId, ValueId>,
    lane0: &Lane,
    cur: BlockId,
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
            Operand::Value(lane0.get(&x).copied().unwrap_or(x)),
        ),
        Operand::ConstF64(_) => (VecShape::F64x2, op),
        _ => (VecShape::I64x2, op),
    };
    let s = vtemp(func, cur, Type::Vec(shape), VecInst::Splat(shape, scalar));
    splats.insert(op, s);
    s
}

/// The end of `b`: step the group again while lane 0 of pair 0 and
/// some other lane still step (as `masked::group_test`).
fn group_test(func: &mut Function, pairs: &[Pair], b: BlockId, fb: BlockId, r: BlockId) {
    let v = Operand::Value;
    let first = new_value(func, Type::I64);
    push(
        func,
        b,
        first,
        InstKind::Vec(VecInst::Lane(0, v(pairs[0].act))),
    );
    let others = match pairs {
        [one] => {
            let hi = new_value(func, Type::I64);
            push(func, b, hi, InstKind::Vec(VecInst::Lane(1, v(one.act))));
            hi
        }
        [p0, p1, rest @ ..] => {
            let mut t = vtemp(func, b, I64X2, VecInst::Ext(v(p0.act), v(p1.act)));
            for pair in std::iter::once(p1).chain(rest) {
                t = vtemp(func, b, I64X2, VecInst::BinOp(VecOp::Or, v(t), v(pair.act)));
            }
            let any = new_value(func, Type::I64);
            push(func, b, any, InstKind::Vec(VecInst::AnyLane(v(t))));
            any
        }
        [] => unreachable!("a packed group has a pair"),
    };
    let both = new_value(func, Type::I64);
    push(
        func,
        b,
        both,
        InstKind::BinOp(BinOp::And, v(first), v(others)),
    );
    let go = new_value(func, Type::Bool);
    push(
        func,
        b,
        go,
        InstKind::ICmp(IPred::Ne, v(both), Operand::ConstI64(0)),
    );
    func.blocks[b.0 as usize].term = Terminator::CondBr {
        cond: Operand::Value(go),
        then_blk: fb,
        else_blk: r,
    };
}

/// `R`: lane 0 of pair 0 as scalars, then `retire` on them.
#[allow(clippy::too_many_arguments)]
fn unpack_slot0(
    func: &mut Function,
    p: &Plan,
    pk: &Packing,
    cells: &HashSet<ValueId>,
    read_after_pre: &HashSet<ValueId>,
    lane0: &Lane,
    pair: &Pair,
    r: BlockId,
    n: BlockId,
) {
    let v = Operand::Value;
    let lane = |func: &mut Function, scalar: ValueId, vec: ValueId| {
        let ty = func.value_type(scalar);
        let x = new_value(func, ty);
        push(func, r, x, InstKind::Vec(VecInst::Lane(0, v(vec))));
        x
    };
    let mut first = lane0.clone();
    let mut slots: Vec<(ValueId, ValueId)> = pair.slots.iter().map(|(s, w)| (*s, *w)).collect();
    slots.sort_by_key(|(s, _)| s.0);
    for (s, vec) in slots {
        let x = lane(func, s, vec);
        first.insert(s, x);
    }
    let mut snap: Lane = Lane::default();
    for c in snapped(func, p) {
        let vec = if pk.frozen.contains(&c) {
            pair.slots[&c]
        } else {
            pair.snaps[&c]
        };
        snap.insert(c, lane(func, c, vec));
    }
    let flag = new_value(func, Type::I64);
    push(func, r, flag, InstKind::Vec(VecInst::Lane(0, v(pair.act))));
    let act = new_value(func, Type::Bool);
    push(
        func,
        r,
        act,
        InstKind::ICmp(IPred::Ne, v(flag), Operand::ConstI64(0)),
    );
    let slot0 = Slot0 {
        first: &first,
        snap: &snap,
        act,
    };
    retire(func, p, cells, read_after_pre, slot0, r, n);
}

/// `N`: every vector takes the next lane along; the last pair's high
/// lane idles with its flag cleared.
fn shift(func: &mut Function, pairs: &[Pair], n: BlockId) {
    let v = Operand::Value;
    // made here, not ahead of the step: it would hold a register
    // through the loop
    let zero = vtemp(
        func,
        n,
        I64X2,
        VecInst::Splat(VecShape::I64x2, Operand::ConstI64(0)),
    );
    for q in 0..pairs.len() {
        let next = pairs.get(q + 1);
        let mut keys: Vec<ValueId> = pairs[q].slots.keys().copied().collect();
        keys.sort_by_key(|k| k.0);
        for k in keys {
            let own = pairs[q].slots[&k];
            let from = next.map_or(own, |p| p.slots[&k]);
            let ty = func.value_type(own);
            let t = vtemp(func, n, ty, VecInst::Ext(v(own), v(from)));
            push(func, n, own, InstKind::Copy(ty, v(t)));
        }
        let mut keys: Vec<ValueId> = pairs[q].snaps.keys().copied().collect();
        keys.sort_by_key(|k| k.0);
        for k in keys {
            let own = pairs[q].snaps[&k];
            let from = next.map_or(own, |p| p.snaps[&k]);
            let ty = func.value_type(own);
            let t = vtemp(func, n, ty, VecInst::Ext(v(own), v(from)));
            push(func, n, own, InstKind::Copy(ty, v(t)));
        }
        let own = pairs[q].act;
        let from = next.map_or(zero, |p| p.act);
        let t = vtemp(func, n, I64X2, VecInst::Ext(v(own), v(from)));
        push(func, n, own, InstKind::Copy(I64X2, v(t)));
    }
}
