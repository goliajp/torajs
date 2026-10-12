//! A pair's divisibility tests and the sums they guard, on the lanes.
//!
//! ```text
//! P:  ...                         P:  sv0 = vec.pair i64x2 s1, s2; sv = copy sv0
//! B:  q = fdiv a, y               B:  qv = vec.fdiv xv, yv
//!     t = fptosi q                    tv = vec.frintz qv
//!     p = mul t, d                    mv = vec.fcmeq tv, qv
//!     c = icmp eq p, n                ...
//!     u = add s, d'                   dv = vec.splat i64x2 d'
//!     w = select c, u, s              av = vec.and mv, dv
//!     s = copy w                      sum = vec.add sv, av
//!     (the same for the other lane)   sv = copy sum
//! X:  ...                         X:  t1 = vec.lane 0 sv; s1 = copy t1; (lane 1 to s2); ...
//! ```
//!
//! With `a = sitofp n`, `y = sitofp d`, `n ∈ [0, 2^53)` and `d ≥ 1`,
//! `trunc(q) == q` exactly when `d` divides `n`: an integral quotient
//! below 2^53 divides exactly, and otherwise the true quotient is at
//! least `1/d` from an integer while the divide errs by less than
//! `2^-53 · n/d < 1/d` (srem_fquot's window and argument; its select
//! rule wrote the scalar chain). `trunc(q) * d == n` holds on the same
//! inputs, so each lane's sum takes the same adds in the same order and
//! wraps the same. The mask lane is all ones or zero, so `mask & d` is
//! `d` or 0.
//!
//! The two sums share one `I64x2` cell, built in the preheader from the
//! lane cells and split back into them at the top of every loop exit:
//! code after the loop reads the scalar cells as before. So the loop
//! may read each sum only in its own chain, and every exit block must
//! be entered from the loop alone.

use crate::id_hash::HashMap;
use crate::interval::NumFact;

use torajs_core::ssa::loop_analysis::NaturalLoop;
use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId, VecInst,
    VecOp, VecShape, visit_value_operands,
};

use super::{Facts, Quot, Site, Tail, inst, new_value, successors};

const F64X2: Type = Type::Vec(VecShape::F64x2);
const I64X2: Type = Type::Vec(VecShape::I64x2);
const F64_EXACT: i128 = 1 << 53;

/// One lane's chain past its conversion, as positions in the block.
struct Lane {
    cvt: usize,
    mul: usize,
    cmp: usize,
    add: usize,
    sel: usize,
    carry: usize,
    /// the sum cell
    s: ValueId,
}

pub(super) struct Chain {
    lanes: [Lane; 2],
    /// the divisor as the first lane's add reads it
    pub dd: ValueId,
    /// the divisor, not written in the block before either select
    pub d: ValueId,
    exits: Vec<BlockId>,
}

impl Chain {
    /// The later select, where the masked add reads `dd`.
    pub(super) fn last_sel(&self) -> usize {
        self.lanes[0].sel.max(self.lanes[1].sel)
    }
}

/// The block the pair sits in and what is known around it.
pub(super) struct Scope<'a> {
    pub func: &'a Function,
    pub b: BlockId,
    pub lp: &'a NaturalLoop,
    pub pre: BlockId,
    pub facts: &'a Facts,
    pub known: &'a HashMap<ValueId, NumFact>,
}

pub(super) fn plan(at: &Scope, (q1, q2): &(Quot, Quot)) -> Option<Chain> {
    let (l1, dd, d1) = lane(at, q1)?;
    let (l2, _, d2) = lane(at, q2)?;
    if l1.s == l2.s || d1 != d2 {
        return None;
    }
    let exits = exits(at.func, at.lp, at.facts)?;
    Some(Chain {
        lanes: [l1, l2],
        dd,
        d: d1,
        exits,
    })
}

/// The quotient's chain to its sum, with the divisor operand of its add
/// and the divisor itself.
fn lane(at: &Scope, q: &Quot) -> Option<(Lane, ValueId, ValueId)> {
    let (func, facts) = (at.func, at.facts);
    let insts = &func.blocks[at.b.0 as usize].insts;
    let t = insts[q.cvt].result?;
    let mul = reader(insts, q.cvt, facts, t)?;
    let InstKind::BinOp(BinOp::Mul, x, y) = &insts[mul].kind else {
        return None;
    };
    let d = other(t, x, y)?;
    let prod = insts[mul].result?;
    let cmp = reader(insts, mul, facts, prod)?;
    let InstKind::ICmp(IPred::Eq, x, y) = &insts[cmp].kind else {
        return None;
    };
    let n = other(prod, x, y)?;
    let c = insts[cmp].result?;
    let sel = reader(insts, cmp, facts, c)?;
    let InstKind::Select(Type::I64, Operand::Value(cc), Operand::Value(u), Operand::Value(s)) =
        insts[sel].kind
    else {
        return None;
    };
    let w = insts[sel].result?;
    let add = insts[..sel].iter().position(|i| i.result == Some(u))?;
    let InstKind::BinOp(BinOp::Add, x, y) = &insts[add].kind else {
        return None;
    };
    let dd = other(s, x, y)?;
    let carry = reader(insts, sel, facts, w)?;
    let carries = matches!(insts[carry].kind, InstKind::Copy(Type::I64, Operand::Value(x)) if x == w)
        && insts[carry].result == Some(s);
    let single = |v: ValueId| facts.single_def(v).is_some() && facts.reads.get(&v) == Some(&1);
    if cc != c
        || !carries
        || !single(u)
        || !(dd == d || copies(insts, facts, dd, d))
        || [s, d, n]
            .iter()
            .any(|v| func.values[v.0 as usize].ty != Type::I64)
        || !sum_kept_in_chain(func, at.lp, s)
        || !same_operands_all_along(at, q, (d, n), sel)
        || !window(func, at.lp, at.known, d, n)
    {
        return None;
    }
    let lane = Lane {
        cvt: q.cvt,
        mul,
        cmp,
        add,
        sel,
        carry,
        s,
    };
    Some((lane, dd, d))
}

/// The operand of a two-operand instruction that is not `v`.
fn other(v: ValueId, x: &Operand, y: &Operand) -> Option<ValueId> {
    match (x, y) {
        (Operand::Value(a), Operand::Value(o)) | (Operand::Value(o), Operand::Value(a))
            if *a == v =>
        {
            Some(*o)
        }
        _ => None,
    }
}

/// The one reader of `v`, after position `from` in the block.
fn reader(insts: &[Inst], from: usize, facts: &Facts, v: ValueId) -> Option<usize> {
    if facts.reads.get(&v) != Some(&1) {
        return None;
    }
    let reads = |i: &Inst| {
        let mut hit = false;
        visit_value_operands(&i.kind, |x| hit |= x == v);
        hit
    };
    insts[from + 1..]
        .iter()
        .position(reads)
        .map(|k| from + 1 + k)
}

/// `dd` is `copy d`, defined once, in this block.
fn copies(insts: &[Inst], facts: &Facts, dd: ValueId, d: ValueId) -> bool {
    facts.single_def(dd).is_some()
        && insts.iter().any(|i| {
            i.result == Some(dd)
                && matches!(i.kind, InstKind::Copy(Type::I64, Operand::Value(x)) if x == d)
        })
}

/// Every instruction defining `v`, by block.
fn defs(func: &Function, v: ValueId) -> impl Iterator<Item = (BlockId, usize, &Inst)> {
    func.blocks.iter().flat_map(move |blk| {
        blk.insts
            .iter()
            .enumerate()
            .filter(move |(_, i)| i.result == Some(v))
            .map(move |(at, i)| (blk.id, at, i))
    })
}

/// In the loop, the sum cell is written only by its carry copy and
/// read only by its add and select.
fn sum_kept_in_chain(func: &Function, lp: &NaturalLoop, s: ValueId) -> bool {
    let (mut writes, mut reads) = (0, 0);
    for b in &lp.body {
        let blk = &func.blocks[b.0 as usize];
        for i in &blk.insts {
            writes += u32::from(i.result == Some(s));
            visit_value_operands(&i.kind, |x| reads += u32::from(x == s));
        }
        if let Terminator::CondBr {
            cond: Operand::Value(x),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(x))) = &blk.term
        {
            reads += u32::from(*x == s);
        }
    }
    writes == 1 && reads == 2
}

/// The divide reads the same `n` and `d` as the multiply, compare and
/// add: `a = sitofp n` in the preheader with `n` written neither after
/// it there nor in the loop, and `y = sitofp d` in this block (or `d`
/// not written in the loop) with `d` not written in this block before
/// the select.
fn same_operands_all_along(at: &Scope, q: &Quot, (d, n): (ValueId, ValueId), sel: usize) -> bool {
    let (func, lp, b, pre) = (at.func, at.lp, at.b, at.pre);
    let mut a_defs = defs(func, q.a);
    let a_ok = matches!(a_defs.next(), Some((ab, a_at, i))
        if ab == pre
            && a_defs.next().is_none()
            && matches!(i.kind, InstKind::SiToFp(Operand::Value(x)) if x == n)
            && !defs(func, n).any(|(nb, n_at, _)| lp.contains(&nb) || nb == pre && n_at > a_at));
    let mut y_defs = defs(func, q.y);
    let y_ok = matches!(y_defs.next(), Some((yb, _, i))
        if y_defs.next().is_none()
            && matches!(i.kind, InstKind::SiToFp(Operand::Value(x)) if x == d)
            && (yb == b || !defs(func, d).any(|(db, _, _)| lp.contains(&db))));
    a_ok && y_ok && !defs(func, d).any(|(db, d_at, _)| db == b && d_at <= sel)
}

/// `n ∈ [0, 2^53)` and `d ≥ 1`, from the interval facts or, for the
/// divisor, a counter stepped by one from a positive constant.
fn window(
    func: &Function,
    lp: &NaturalLoop,
    known: &HashMap<ValueId, NumFact>,
    d: ValueId,
    n: ValueId,
) -> bool {
    let within =
        |v: ValueId, lo: i128, hi: i128| known.get(&v).is_some_and(|f| f.lo >= lo && f.hi < hi);
    let mut n_defs = defs(func, n);
    let n_ok = within(n, 0, F64_EXACT)
        || matches!((n_defs.next(), n_defs.next()), (Some((_, _, i)), None)
            if matches!(i.kind, InstKind::BinOp(BinOp::Add, Operand::Value(m), Operand::ConstI64(k))
                if k >= 0 && within(m, 0, F64_EXACT - i128::from(k))));
    n_ok && (within(d, 1, i128::MAX) || counts_up_from_one(func, lp, d))
}

/// `d` enters the loop as a constant ≥ 1 and the loop only adds 1 to
/// it: it cannot wrap before 2^63 trips.
fn counts_up_from_one(func: &Function, lp: &NaturalLoop, d: ValueId) -> bool {
    defs(func, d).all(|(db, _, i)| match i.kind {
        InstKind::Copy(Type::I64, Operand::ConstI64(c)) => !lp.contains(&db) && c >= 1,
        InstKind::Copy(Type::I64, Operand::Value(w)) if lp.contains(&db) => {
            let mut w_defs = defs(func, w);
            matches!((w_defs.next(), w_defs.next()), (Some((_, _, wi)), None)
                if matches!(wi.kind, InstKind::BinOp(BinOp::Add, Operand::Value(x), Operand::ConstI64(1)) if x == d))
        }
        _ => false,
    })
}

/// The blocks the loop exits to, when only the loop enters them.
fn exits(func: &Function, lp: &NaturalLoop, facts: &Facts) -> Option<Vec<BlockId>> {
    let mut out: Vec<BlockId> = Vec::new();
    for b in &lp.body {
        for e in successors(&func.blocks[b.0 as usize].term) {
            if lp.contains(&e) || out.contains(&e) {
                continue;
            }
            if !facts.preds.get(&e)?.iter().all(|p| lp.contains(p)) {
                return None;
            }
            out.push(e);
        }
    }
    Some(out)
}

/// Registers the chain's sites, builds the sum vector in the preheader
/// and splits it at the exits; what the divide site emits after the
/// vector divide.
pub(super) fn place(
    func: &mut Function,
    pre: BlockId,
    chain: &Chain,
    sites: &mut HashMap<usize, Site>,
) -> Tail {
    let v = Operand::Value;
    let [tv, mv] = [new_value(func, F64X2), new_value(func, I64X2)];
    let [sv0, sv, sum] = [(); 3].map(|_| new_value(func, I64X2));
    let [l1, l2] = &chain.lanes;
    let entry = VecInst::Pair(VecShape::I64x2, v(l1.s), v(l2.s));
    let pre_insts = &mut func.blocks[pre.0 as usize].insts;
    pre_insts.push(inst(sv0, InstKind::Vec(entry)));
    pre_insts.push(inst(sv, InstKind::Copy(I64X2, v(sv0))));
    for l in &chain.lanes {
        for at in [l.cvt, l.mul, l.cmp, l.add] {
            sites.insert(at, Site::Gone);
        }
    }
    let (first, last) = (l1.sel.min(l2.sel), l1.sel.max(l2.sel));
    sites.insert(first, Site::Gone);
    sites.insert(last, Site::Accumulate(chain.dd, mv, sv, sum));
    let (first, last) = (l1.carry.min(l2.carry), l1.carry.max(l2.carry));
    sites.insert(first, Site::Gone);
    sites.insert(last, Site::Carry(sv, sum));
    for e in &chain.exits {
        let mut split = Vec::with_capacity(4);
        for (k, l) in chain.lanes.iter().enumerate() {
            let t = new_value(func, Type::I64);
            split.push(inst(t, InstKind::Vec(VecInst::Lane(k as u8, v(sv)))));
            split.push(inst(l.s, InstKind::Copy(Type::I64, v(t))));
        }
        func.blocks[e.0 as usize].insts.splice(0..0, split);
    }
    Tail::Mask(tv, mv)
}

/// The masked add at the pair's later select, the divisor's splat first
/// when this is the block's first use of it.
pub(super) fn emit_add(
    func: &mut Function,
    out: &mut Vec<Inst>,
    splats: &mut HashMap<ValueId, ValueId>,
    (dd, mv, sv, sum): (ValueId, ValueId, ValueId, ValueId),
) {
    let v = Operand::Value;
    let dv = match splats.get(&dd) {
        Some(dv) => *dv,
        None => {
            let dv = new_value(func, I64X2);
            let splat = VecInst::Splat(VecShape::I64x2, v(dd));
            out.push(inst(dv, InstKind::Vec(splat)));
            splats.insert(dd, dv);
            dv
        }
    };
    let av = new_value(func, I64X2);
    out.push(inst(
        av,
        InstKind::Vec(VecInst::BinOp(VecOp::And, v(mv), v(dv))),
    ));
    out.push(inst(
        sum,
        InstKind::Vec(VecInst::BinOp(VecOp::Add, v(sv), v(av))),
    ));
}

#[cfg(test)]
mod tests;
