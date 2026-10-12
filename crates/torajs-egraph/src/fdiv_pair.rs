//! Two float quotients of one divisor in one loop block, each read
//! only by its `fptosi`, taken from one two-lane divide.
//!
//! ```text
//! P:  ...                                 P:  xv = pair a1, a2
//! B:  q1 = fdiv a1, y                     B:  yv = splat y
//!     t1 = fptosi q1                          qv = vec.fdiv xv, yv
//!     ...                                     tv = vec.fptosi qv
//!     q2 = fdiv a2, y                         t1 = lane 0 tv
//!     t2 = fptosi q2                          ...
//!                                             t2 = lane 1 tv
//! ```
//!
//! SLP pairing of isomorphic independent chains (Larsen & Amarasinghe
//! 2000), restricted to dividends defined outside the loop so their
//! lane pair is built once in the preheader. The M-series f64 divider
//! takes a two-lane divide at the one-lane rate, so a block bound by
//! its divides (unroll_jam's lanes of a trial-division loop, each
//! dividing its own `n` by the shared `d`) runs two quotients per
//! issue. Each lane divides and converts exactly as the scalar
//! instructions did, so the values are the same bit for bit.
//!
//! When both quotients feed a divisibility test that adds the divisor
//! to a per-lane sum (`if (n % d === 0) s += d`, after srem_fquot), the
//! test and the sum move onto the lanes as well (`accum`).
//!
//! A divisor that is a loop counter is read from vector cells stepped
//! beside it instead of splatted every trip (`counter`).
//!
//! After unroll_jam and unroll_pair, before loop_rotate.
//! `TORAJS_FDIV_PAIR_OFF=1` skips, `TORAJS_FDIV_PAIR_STATS=1` counts.

mod accum;
mod counter;

use crate::id_hash::HashMap;
use crate::interval::NumFact;

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::{LoopAnalysis, NaturalLoop};
use torajs_core::ssa::{
    BinOp, BlockId, Function, Inst, InstKind, Module, Operand, Terminator, Type, ValueId,
    ValueInfo, VecInst, VecOp, VecShape, visit_value_operands,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FdivPairStats {
    /// quotient pairs taken from one two-lane divide
    pub pairs: u32,
    /// of those, the pairs whose divisibility test and masked sum run
    /// on the lanes too
    pub accumulated: u32,
    /// counter divisors whose f64 lanes come from a stepped cell
    pub float_counters: u32,
    /// counter divisors whose i64 lanes come from a stepped cell
    pub int_counters: u32,
}

pub fn pair_module(
    module: &mut Module,
    facts: Option<&[HashMap<ValueId, NumFact>]>,
) -> FdivPairStats {
    debug_assert!(facts.is_none_or(|f| f.len() == module.funcs.len()));
    let mut stats = FdivPairStats::default();
    for (fi, func) in module.funcs.iter_mut().enumerate() {
        if !func.is_declaration() && has_two_fdivs(func) {
            pair_function(func, facts.map(|f| &f[fi]), &mut stats);
        }
    }
    stats
}

fn has_two_fdivs(func: &Function) -> bool {
    func.blocks.iter().any(|b| {
        b.insts
            .iter()
            .filter(|i| matches!(i.kind, InstKind::BinOp(BinOp::FDiv, ..)))
            .nth(1)
            .is_some()
    })
}

/// A scalar `q = fdiv a, y` at `fdiv`, read only by `t = fptosi q`
/// at `cvt` in the same block.
#[derive(Clone, Copy)]
struct Quot {
    fdiv: usize,
    cvt: usize,
    a: ValueId,
    y: ValueId,
}

struct Facts {
    /// def sites per value
    defs: HashMap<ValueId, Vec<(BlockId, usize)>>,
    /// number of reads per value, terminators included
    reads: HashMap<ValueId, u32>,
    preds: HashMap<BlockId, Vec<BlockId>>,
}

impl Facts {
    fn of(func: &Function) -> Self {
        let mut f = Facts {
            defs: HashMap::default(),
            reads: HashMap::default(),
            preds: HashMap::default(),
        };
        for block in &func.blocks {
            for s in successors(&block.term) {
                f.preds.entry(s).or_default().push(block.id);
            }
            for (i, inst) in block.insts.iter().enumerate() {
                if let Some(r) = inst.result {
                    f.defs.entry(r).or_default().push((block.id, i));
                }
                visit_value_operands(&inst.kind, |v| *f.reads.entry(v).or_default() += 1);
            }
            if let Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) = &block.term
            {
                *f.reads.entry(*v).or_default() += 1;
            }
        }
        f
    }

    fn single_def(&self, v: ValueId) -> Option<(BlockId, usize)> {
        match self.defs.get(&v)?.as_slice() {
            [d] => Some(*d),
            _ => None,
        }
    }
}

fn successors(term: &Terminator) -> Vec<BlockId> {
    match term {
        Terminator::Br(t) => vec![*t],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
        Terminator::Ret(_) | Terminator::Unreachable => vec![],
    }
}

/// Pairs per loop. Each pair's dividend vector is built in the
/// preheader and lives through the whole loop, and a vector value
/// cannot spill; past this many the FP registers would run out.
const PAIRS_PER_LOOP_MAX: usize = 4;

fn pair_function(
    func: &mut Function,
    known: Option<&HashMap<ValueId, NumFact>>,
    stats: &mut FdivPairStats,
) {
    let dom = DominatorTree::compute(func);
    let la = LoopAnalysis::compute(func, &dom);
    let facts = Facts::of(func);
    let mut per_loop: HashMap<BlockId, usize> = HashMap::default();
    for b in 0..func.blocks.len() as u32 {
        let b = BlockId(b);
        let Some(lp) = la
            .loops()
            .iter()
            .filter(|l| l.contains(&b))
            .min_by_key(|l| l.body.len())
        else {
            continue;
        };
        let Some(pre) = preheader(func, lp, &facts) else {
            continue;
        };
        let mut pairs = plan_block(func, b, lp, &facts);
        let used = per_loop.entry(lp.header).or_default();
        pairs.truncate(PAIRS_PER_LOOP_MAX - *used);
        *used += pairs.len();
        if !pairs.is_empty() {
            let chains: Vec<Option<accum::Chain>> = match known {
                Some(known) => {
                    let at = accum::Scope {
                        func,
                        b,
                        lp,
                        pre,
                        facts: &facts,
                        known,
                    };
                    pairs.iter().map(|p| accum::plan(&at, p)).collect()
                }
                None => pairs.iter().map(|_| None).collect(),
            };
            stats.pairs += pairs.len() as u32;
            stats.accumulated += chains.iter().flatten().count() as u32;
            let counters = known
                .map(|known| counter::plan(func, b, lp, known, &pairs, &chains))
                .unwrap_or_default();
            stats.float_counters += counters.float.len() as u32;
            stats.int_counters += counters.int.len() as u32;
            let cells = counter::build_cells(func, pre, &counters);
            rewrite_block(func, b, pre, &pairs, &chains, &cells);
            for (d, cs) in &cells.by_counter {
                counter::step_cells(func, lp, b, *d, cs);
            }
            counter::drop_unread(func, b, &cells);
        }
    }
}

/// The loop's single way in from outside, a plain jump to the header.
fn preheader(func: &Function, lp: &NaturalLoop, facts: &Facts) -> Option<BlockId> {
    let outside: Vec<BlockId> = facts
        .preds
        .get(&lp.header)?
        .iter()
        .copied()
        .filter(|p| !lp.contains(p))
        .collect();
    match outside.as_slice() {
        [p] if matches!(func.blocks[p.0 as usize].term, Terminator::Br(_)) => Some(*p),
        _ => None,
    }
}

/// The block's quotients paired in program order, per shared divisor.
fn plan_block(func: &Function, b: BlockId, lp: &NaturalLoop, facts: &Facts) -> Vec<(Quot, Quot)> {
    let insts = &func.blocks[b.0 as usize].insts;
    let mut by_divisor: HashMap<ValueId, Vec<Quot>> = HashMap::default();
    let mut order: Vec<ValueId> = Vec::new();
    for (at, inst) in insts.iter().enumerate() {
        let (Some(q), InstKind::BinOp(BinOp::FDiv, Operand::Value(a), Operand::Value(y))) =
            (inst.result, &inst.kind)
        else {
            continue;
        };
        if func.values[q.0 as usize].ty != Type::F64
            || facts.single_def(q).is_none()
            || facts.reads.get(&q) != Some(&1)
            || facts.defs.get(y).is_some_and(|d| d.len() != 1)
        {
            continue;
        }
        // the dividend is fixed for the whole loop entry: a parameter
        // or one def outside the loop
        match facts.defs.get(a).map(Vec::as_slice) {
            None => {}
            Some([(db, _)]) if !lp.contains(db) => {}
            _ => continue,
        }
        let Some(cvt) = insts[at + 1..]
            .iter()
            .position(|i| matches!(i.kind, InstKind::FpToSi(Operand::Value(x)) if x == q))
        else {
            continue;
        };
        let quot = Quot {
            fdiv: at,
            cvt: at + 1 + cvt,
            a: *a,
            y: *y,
        };
        let group = by_divisor.entry(*y).or_default();
        if group.is_empty() {
            order.push(*y);
        }
        group.push(quot);
    }
    let pair = |c: &[Quot; 2]| (c[0], c[1]);
    let pairs = |y| by_divisor[y].as_chunks().0.iter().map(pair);
    order.iter().flat_map(pairs).collect()
}

fn new_value(func: &mut Function, ty: Type) -> ValueId {
    let id = ValueId(func.values.len() as u32);
    func.values.push(ValueInfo { ty, name: None });
    id
}

fn inst(result: ValueId, kind: InstKind) -> Inst {
    Inst {
        result: Some(result),
        kind,
        origin: None,
    }
}

/// What the rewritten block puts at an old instruction's position.
enum Site {
    /// the first divide: the shared splat (when this pair is the
    /// divisor's first), the vector divide and what follows it
    Divide(Option<ValueId>, ValueId, ValueId, Tail),
    /// a conversion: its result from lane `k`
    Lane(u8, ValueId),
    /// the masked add of an accumulating pair, at its later select:
    /// the divisor operand, the mask, the sum cell and the new sum
    Accumulate(ValueId, ValueId, ValueId, ValueId),
    /// the sum cell's write, at the pair's later carry copy
    Carry(ValueId, ValueId),
    /// the second divide, now part of the first's vector; and every
    /// scalar step an accumulating pair replaces
    Gone,
}

/// What follows a pair's vector divide.
enum Tail {
    /// `vec.fptosi` into the lane vector the conversions read
    Convert(ValueId),
    /// `vec.frintz` into the first, `vec.fcmeq` of it and the quotient
    /// into the second
    Mask(ValueId, ValueId),
}

fn rewrite_block(
    func: &mut Function,
    b: BlockId,
    pre: BlockId,
    pairs: &[(Quot, Quot)],
    chains: &[Option<accum::Chain>],
    cells: &counter::Cells,
) {
    const F64X2: Type = Type::Vec(VecShape::F64x2);
    const I64X2: Type = Type::Vec(VecShape::I64x2);
    let v = Operand::Value;
    let mut sites: HashMap<usize, Site> = HashMap::default();
    // a counter divisor's lanes are its cell, never splatted
    let mut splats = cells.float.clone();
    for ((q1, q2), chain) in pairs.iter().zip(chains) {
        let xv = new_value(func, F64X2);
        let pair = InstKind::Vec(VecInst::Pair(VecShape::F64x2, v(q1.a), v(q2.a)));
        func.blocks[pre.0 as usize].insts.push(inst(xv, pair));
        let fresh = !splats.contains_key(&q1.y);
        if fresh {
            let yv = new_value(func, F64X2);
            splats.insert(q1.y, yv);
        }
        let qv = new_value(func, F64X2);
        let splat = fresh.then(|| splats[&q1.y]);
        sites.insert(q2.fdiv, Site::Gone);
        if let Some(chain) = chain {
            let tail = accum::place(func, pre, chain, &mut sites);
            sites.insert(q1.fdiv, Site::Divide(splat, xv, qv, tail));
            continue;
        }
        let tv = new_value(func, I64X2);
        sites.insert(q1.fdiv, Site::Divide(splat, xv, qv, Tail::Convert(tv)));
        sites.insert(q1.cvt, Site::Lane(0, tv));
        sites.insert(q2.cvt, Site::Lane(1, tv));
    }
    let mut dsplats = cells.int.clone();
    let old = std::mem::take(&mut func.blocks[b.0 as usize].insts);
    let mut out = Vec::with_capacity(old.len() + 2 * pairs.len());
    // what follows a divide waits until the next instruction that is
    // neither a divide nor gone, so neighbouring divides issue back to
    // back: the divider takes the second while the first's quotient is
    // still in flight, instead of the second queueing behind the first
    // one's dependent rounding and compare
    let mut tails: Vec<Inst> = Vec::new();
    for (at, i) in old.into_iter().enumerate() {
        let site = sites.get(&at);
        if !matches!(site, Some(Site::Divide(..) | Site::Gone)) {
            out.append(&mut tails);
        }
        match site {
            None => out.push(i),
            Some(Site::Gone) => {}
            Some(Site::Lane(k, tv)) => out.push(Inst {
                result: i.result,
                kind: InstKind::Vec(VecInst::Lane(*k, v(*tv))),
                origin: i.origin,
            }),
            Some(Site::Accumulate(dd, mv, sv, sum)) => {
                accum::emit_add(func, &mut out, &mut dsplats, (*dd, *mv, *sv, *sum));
            }
            Some(Site::Carry(sv, sum)) => {
                let kind = InstKind::Copy(I64X2, v(*sum));
                out.push(inst(*sv, kind));
            }
            Some(Site::Divide(splat, xv, qv, tail)) => {
                let InstKind::BinOp(_, _, y) = i.kind else {
                    unreachable!("a planned divide is an fdiv");
                };
                let Operand::Value(yid) = y else {
                    unreachable!("a planned divisor is a value");
                };
                let yv = splats[&yid];
                if let Some(s) = splat {
                    out.push(inst(*s, InstKind::Vec(VecInst::Splat(VecShape::F64x2, y))));
                }
                out.push(Inst {
                    result: Some(*qv),
                    kind: InstKind::Vec(VecInst::BinOp(VecOp::FDiv, v(*xv), v(yv))),
                    origin: i.origin,
                });
                match tail {
                    Tail::Convert(tv) => {
                        tails.push(inst(*tv, InstKind::Vec(VecInst::FpToSi(v(*qv)))));
                    }
                    Tail::Mask(tv, mv) => {
                        tails.push(inst(*tv, InstKind::Vec(VecInst::FRintZ(v(*qv)))));
                        let eq = VecInst::BinOp(VecOp::FCmEq, v(*tv), v(*qv));
                        tails.push(inst(*mv, InstKind::Vec(eq)));
                    }
                }
            }
        }
    }
    out.append(&mut tails);
    func.blocks[b.0 as usize].insts = out;
}

#[cfg(test)]
mod tests;
