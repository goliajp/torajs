//! The group path for an inner loop with an early exit (a `break`, an
//! inlined `return`): its lanes leave it at different steps, so the
//! group steps every lane under a mask instead of stopping at the
//! first lane done.
//!
//! ```text
//! GB:  pre for every lane (as unmasked); lane slots hold each lane's
//!      cells and pre values; snap_j = the cells the inner
//!      loop writes (or fewer, see `retire`); act_j = true; live = L
//! FB:  every slot's inner step as it is; go_j = act_j and every exit
//!      test the step passes; snap_j = go_j ? cells_j : snap_j;
//!      act_j = go_j ; cond_br act_0 and any other act_j, FB, R
//! R:   original cells = slot 0's snap and cells ; br T (`retire`; the
//!      counter is slot 0's already)
//! T:   clone of inner loop + post + latch; latch → N
//! N:   slot k = slot k+1, the last slot idle; live -= 1
//!      cond_br live > 0, FT, G
//! FT:  cond_br act_0 and any other act_j, FB, R
//! ```
//!
//! Why the result cannot differ:
//! - a slot whose step leaves keeps in `snap` the cells it started that
//!   step with, and every later step leaves them alone; `T` reruns that
//!   step from them and takes the same exit (the inner loop is pure).
//!   When the exits read fewer cells than the step writes, only those
//!   are kept and `retire` replays the exit tests instead.
//!   The cells themselves run on, so the select sits off the step's
//!   own recurrence.
//! - slot 0 always holds the earliest lane not yet finished; the group
//!   path runs only while it is still stepping, so it never runs longer
//!   than that lane's own inner loop, and the lanes' `post` and latch
//!   run one at a time in iteration order. Left stepping alone, slot 0
//!   goes on in `T` from the cells it holds (its snapshot then is its
//!   current state).
//! - the other slots' steps are pure and trap-free (no integer divide
//!   is masked), and read no value `post` writes.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Operand, Terminator, Type, ValueId,
    visit_value_operands,
};

use super::clone::{clone_into, new_value, push};
use super::emit::reuse_shared;
use super::retire::{Slot0, retire, snapped};
use super::{LANE_GPR_BUDGET, MIN_LANES, Plan};

type Lane = HashMap<ValueId, ValueId>;

/// Float registers a masked lane's state may take, out of the 21 the
/// allocator hands out; the rest hold shared values and each step's
/// temporaries.
const LANE_FPR_BUDGET: usize = 16;

/// Lanes for a masked group: a step that packs two lanes to a vector
/// takes as many pairs as fit (`masked_vec::packing`); otherwise the default
/// count, or fewer where every lane's state (its slot values, its
/// snapshots and its flag) would no longer fit in registers; spilled
/// lane state costs a load and a store each step. At least two.
pub(super) fn masked_lanes(func: &Function, p: &Plan) -> usize {
    if let Some(pk) = &p.packing {
        return pk.lanes;
    }
    let read_after_pre = p
        .read_after_pre
        .as_ref()
        .expect("a masked plan has its reads past pre");
    let kept = snapped(func, p);
    let mut own: HashSet<ValueId> = HashSet::default();
    for b in p.pre_order() {
        let results = func.blocks[b.0 as usize]
            .insts
            .iter()
            .filter_map(|i| i.result);
        own.extend(
            results.filter(|r| {
                Some(*r) != p.skip && !p.shared.contains(r) && read_after_pre.contains(r)
            }),
        );
    }
    let state = own
        .iter()
        .chain(&kept)
        .map(|v| func.values[v.0 as usize].ty)
        .chain([Type::Bool]);
    let (mut fprs, mut gprs) = (0, 0);
    for ty in state {
        if ty == Type::F64 {
            fprs += 1;
        } else {
            gprs += 1;
        }
    }
    let fit = (LANE_FPR_BUDGET / fprs.max(1)).min(LANE_GPR_BUDGET / gprs);
    fit.clamp(2, MIN_LANES)
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
    if p.packing.is_some() {
        return super::masked_vec::steps(func, p, cells, read_after_pre, g, cur, lanes);
    }
    let slots = slot_values(p, cells, read_after_pre, &lanes[0]);
    for lane in lanes.iter_mut() {
        for &v in &slots {
            // a lane cell is its own slot
            if cells.contains(&v) {
                continue;
            }
            let slot = copy_of(func, cur, lane.get(&v).copied().unwrap_or(v));
            lane.insert(v, slot);
        }
    }
    let kept = snapped(func, p);
    let mut snaps: Vec<Lane> = Vec::with_capacity(p.lanes);
    for lane in &lanes {
        let snap = kept.iter().map(|c| (*c, copy_of(func, cur, lane[c])));
        snaps.push(snap.collect());
    }
    let act: Vec<ValueId> = (0..p.lanes).map(|_| new_value(func, Type::Bool)).collect();
    for &a in &act {
        push(
            func,
            cur,
            a,
            InstKind::Copy(Type::Bool, Operand::ConstBool(true)),
        );
    }
    let live = new_value(func, Type::I64);
    let lanes_left = Operand::ConstI64(p.lanes as i64);
    push(func, cur, live, InstKind::Copy(Type::I64, lanes_left));
    let fb = func.add_block();
    let r = func.add_block();
    let n = func.add_block();
    let ft = func.add_block();
    func.blocks[cur.0 as usize].term = Terminator::Br(fb);
    // the tail reads slot 0's pre values, not what the steps compute
    let first = lanes[0].clone();
    let (lane0, rest) = lanes.split_at_mut(1);
    let mut reads_iv = false;
    for b in p.inner_order() {
        for inst in &func.blocks[b.0 as usize].insts {
            visit_value_operands(&inst.kind, |v| reads_iv |= v == p.iv);
        }
    }
    lane_step(func, p, fb, act[0], &mut lane0[0], None, &snaps[0]);
    for (j, lane) in rest.iter_mut().enumerate() {
        // slot k's lane is the k-th after slot 0's, whose counter is the
        // loop's own: the tail's latch steps it as each lane retires
        if reads_iv {
            let off = Operand::ConstI64(p.step * (j as i64 + 1));
            let ik = new_value(func, Type::I64);
            push(
                func,
                fb,
                ik,
                InstKind::BinOp(BinOp::Add, Operand::Value(p.iv), off),
            );
            lane.insert(p.iv, ik);
        }
        lane_step(
            func,
            p,
            fb,
            act[j + 1],
            lane,
            Some(&lane0[0]),
            &snaps[j + 1],
        );
    }
    group_test(func, &act, fb, fb, r);
    group_test(func, &act, ft, fb, r);
    let slot0 = Slot0 {
        first: &first,
        snap: &snaps[0],
        act: act[0],
    };
    retire(func, p, cells, read_after_pre, slot0, r, n);
    shift(func, n, &slots, &lanes);
    shift(func, n, &kept, &snaps);
    next_slot(func, p, n, &act, live, ft, g);
}

/// `N`'s own part: every flag moves down a slot, the last slot idles,
/// and the group goes on while a lane is left.
fn next_slot(
    func: &mut Function,
    p: &Plan,
    n: BlockId,
    act: &[ValueId],
    live: ValueId,
    ft: BlockId,
    g: BlockId,
) {
    for k in 0..p.lanes - 1 {
        push(
            func,
            n,
            act[k],
            InstKind::Copy(Type::Bool, Operand::Value(act[k + 1])),
        );
    }
    let idle = Operand::ConstBool(false);
    push(func, n, act[p.lanes - 1], InstKind::Copy(Type::Bool, idle));
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

/// The end of `b`: step the group again while slot 0 and some other
/// slot still step. Slot 0 done retires it; slot 0 alone finishes its
/// inner loop in the tail, at its own pace, instead of carrying idle
/// slots.
fn group_test(func: &mut Function, act: &[ValueId], b: BlockId, fb: BlockId, r: BlockId) {
    let mut others = Operand::Value(act[1]);
    for &a in &act[2..] {
        let any = new_value(func, Type::Bool);
        let kind = InstKind::Select(
            Type::Bool,
            Operand::Value(a),
            Operand::ConstBool(true),
            others,
        );
        push(func, b, any, kind);
        others = Operand::Value(any);
    }
    let go = new_value(func, Type::Bool);
    let off = Operand::ConstBool(false);
    push(
        func,
        b,
        go,
        InstKind::Select(Type::Bool, Operand::Value(act[0]), others, off),
    );
    func.blocks[b.0 as usize].term = Terminator::CondBr {
        cond: Operand::Value(go),
        then_blk: fb,
        else_blk: r,
    };
}

/// A new cell in `b` holding `v`.
pub(super) fn copy_of(func: &mut Function, b: BlockId, v: ValueId) -> ValueId {
    let ty = func.values[v.0 as usize].ty;
    let cell = new_value(func, ty);
    push(func, b, cell, InstKind::Copy(ty, Operand::Value(v)));
    cell
}

/// What a slot holds for its lane: its own cells and pre values read
/// past `pre`. Shared values are every lane's; a lane's counter is
/// slot 0's plus its distance.
pub(super) fn slot_values(
    p: &Plan,
    cells: &HashSet<ValueId>,
    read_after_pre: &HashSet<ValueId>,
    lane0: &Lane,
) -> Vec<ValueId> {
    let mut out: Vec<ValueId> = lane0
        .keys()
        .copied()
        .filter(|v| {
            *v != p.iv
                && read_after_pre.contains(v)
                && !p.shared.contains(v)
                && (!cells.contains(v) || p.lane_cells.contains(v))
        })
        .collect();
    out.sort_by_key(|v| v.0);
    out
}

/// One slot's inner step in `fb`, header and body cloned in order; each
/// exit test it passes keeps `go` true. Then `snap` takes the cells
/// the step left when it went on, and `act` takes `go`.
fn lane_step(
    func: &mut Function,
    p: &Plan,
    fb: BlockId,
    act: ValueId,
    lane: &mut Lane,
    lane0: Option<&Lane>,
    snap: &Lane,
) {
    let inner = p.inner_order();
    let mut go = Operand::Value(act);
    for &b in &inner {
        let block = func.blocks[b.0 as usize].clone();
        for inst in &block.insts {
            if !reuse_shared(p, inst, lane0, lane) {
                clone_into(func, fb, inst, lane);
            }
        }
        if let Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            ..
        } = block.term
        {
            let cond = Operand::Value(lane.get(&c).copied().unwrap_or(c));
            let off = Operand::ConstBool(false);
            let kind = if inner.contains(&then_blk) {
                InstKind::Select(Type::Bool, cond, go, off)
            } else {
                InstKind::Select(Type::Bool, cond, off, go)
            };
            let flag = new_value(func, Type::Bool);
            push(func, fb, flag, kind);
            go = Operand::Value(flag);
        }
    }
    let mut snapped: Vec<(&ValueId, &ValueId)> = snap.iter().collect();
    snapped.sort_by_key(|(c, _)| c.0);
    for (c, s) in snapped {
        let ty = func.values[c.0 as usize].ty;
        let now = Operand::Value(lane[c]);
        let kept = new_value(func, ty);
        push(
            func,
            fb,
            kept,
            InstKind::Select(ty, go, now, Operand::Value(*s)),
        );
        push(func, fb, *s, InstKind::Copy(ty, Operand::Value(kept)));
    }
    push(func, fb, act, InstKind::Copy(Type::Bool, go));
}

/// `N`: slot k takes slot k+1's lane in each of `maps`, for `keys`.
pub(super) fn shift(func: &mut Function, n: BlockId, keys: &[ValueId], maps: &[Lane]) {
    for k in 0..maps.len() - 1 {
        for &v in keys {
            let ty = func.values[v.0 as usize].ty;
            let next = Operand::Value(maps[k + 1][&v]);
            push(func, n, maps[k][&v], InstKind::Copy(ty, next));
        }
    }
}
