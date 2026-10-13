//! Where a packed group's lanes go between steps: the group test, slot
//! 0 unpacked for `retire`, and the shift of every vector one lane
//! along.

use crate::id_hash::HashSet;

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Operand, Terminator, Type, ValueId, VecInst, VecOp,
    VecShape,
};

use super::super::clone::{new_value, push};
use super::super::retire::{Slot0, retire, snapped};
use super::{Group, I64X2, Lane, Pair, vtemp};

/// The end of `b`: step the group again while lane 0 of pair 0 and
/// some other lane still step (as `masked::group_test`).
pub(super) fn group_test(func: &mut Function, pairs: &[Pair], b: BlockId, fb: BlockId, r: BlockId) {
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

/// `R`: lane 0 of pair 0 as scalars beside slot 0's scalar slots, then
/// `retire` on them, leaving for `n`.
pub(super) fn unpack_slot0(
    func: &mut Function,
    group: &Group,
    cells: &HashSet<ValueId>,
    read_after_pre: &HashSet<ValueId>,
    pair: &Pair,
    (r, n): (BlockId, BlockId),
) {
    let v = Operand::Value;
    let lane = |func: &mut Function, scalar: ValueId, vec: ValueId| {
        let ty = func.value_type(scalar);
        let x = new_value(func, ty);
        push(func, r, x, InstKind::Vec(VecInst::Lane(0, v(vec))));
        x
    };
    let mut first = group.lane0.clone();
    let mut slots: Vec<(ValueId, ValueId)> = pair.slots.iter().map(|(s, w)| (*s, *w)).collect();
    slots.sort_by_key(|(s, _)| s.0);
    for (s, vec) in slots {
        let x = lane(func, s, vec);
        first.insert(s, x);
    }
    let mut snap: Lane = Lane::default();
    for c in snapped(func, group.p) {
        let vec = if group.pk.is_frozen(c) {
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
    retire(func, group.p, cells, read_after_pre, slot0, r, n);
}

/// `N`: every vector takes the next lane along; the last pair's high
/// lane idles with its flag cleared.
pub(super) fn shift(func: &mut Function, pairs: &[Pair], n: BlockId) {
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
