//! Which loops the rewrite takes.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::{LoopAnalysis, NaturalLoop};
use torajs_core::ssa::{
    BinOp, BlockId, Function, InstKind, Operand, Terminator, Type, ValueId, visit_value_operands,
};

use super::{MAX_CHAIN_BLOCKS, MAX_INSTS, MAX_STEP, Plan};

/// Where each value is defined and read, built once per function; a
/// terminator reads at index `insts.len()`.
pub(super) struct Uses {
    pub defs: HashMap<ValueId, Vec<(BlockId, usize)>>,
    pub reads: HashMap<ValueId, Vec<(BlockId, usize)>>,
    pub preds: HashMap<BlockId, Vec<BlockId>>,
}

impl Uses {
    pub fn of(func: &Function) -> Self {
        let mut u = Uses {
            defs: HashMap::default(),
            reads: HashMap::default(),
            preds: HashMap::default(),
        };
        for block in &func.blocks {
            for s in successors(&block.term) {
                u.preds.entry(s).or_default().push(block.id);
            }
            for (i, inst) in block.insts.iter().enumerate() {
                if let Some(r) = inst.result {
                    u.defs.entry(r).or_default().push((block.id, i));
                }
                visit_value_operands(&inst.kind, |v| {
                    u.reads.entry(v).or_default().push((block.id, i));
                });
            }
            if let Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) = &block.term
            {
                u.reads
                    .entry(*v)
                    .or_default()
                    .push((block.id, block.insts.len()));
            }
        }
        u
    }

    pub fn single_def(&self, v: ValueId) -> Option<(BlockId, usize)> {
        match self.defs.get(&v)?.as_slice() {
            [d] => Some(*d),
            _ => None,
        }
    }

    /// Values every def of which is a `Copy`.
    pub fn cells(&self, func: &Function) -> HashSet<ValueId> {
        self.defs
            .iter()
            .filter(|(_, ds)| {
                ds.iter().all(|(b, i)| {
                    matches!(func.blocks[b.0 as usize].insts[*i].kind, InstKind::Copy(..))
                })
            })
            .map(|(v, _)| *v)
            .collect()
    }
}

pub(super) fn successors(term: &Terminator) -> Vec<BlockId> {
    match term {
        Terminator::Br(t) => vec![*t],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
        Terminator::Ret(_) | Terminator::Unreachable => vec![],
    }
}

pub(super) fn find_plan(func: &Function, done: &HashSet<BlockId>) -> Option<Plan> {
    find_plan_where(func, done, |p, _, _| Some(p))
}

/// The first loop the rewrite takes that `accept` also takes, given
/// the loop and the uses it was planned against.
pub(super) fn find_plan_where<T>(
    func: &Function,
    done: &HashSet<BlockId>,
    mut accept: impl FnMut(Plan, &NaturalLoop, &Uses) -> Option<T>,
) -> Option<T> {
    let dom = DominatorTree::compute(func);
    let la = LoopAnalysis::compute(func, &dom);
    let uses = Uses::of(func);
    la.loops()
        .iter()
        .filter(|l| !done.contains(&l.header))
        .find_map(|l| accept(plan_loop(func, &la, l, &uses)?, l, &uses))
}

pub(super) fn blk(func: &Function, b: BlockId) -> &torajs_core::ssa::Block {
    &func.blocks[b.0 as usize]
}

/// Pure, trap-free, reads no memory: what both copies may run early
/// and what a clone may repeat.
fn pure(kind: &InstKind) -> bool {
    match kind {
        InstKind::BinOp(op, _, _) => *op != BinOp::FRem,
        InstKind::ICmp(..)
        | InstKind::FCmp(..)
        | InstKind::Copy(..)
        | InstKind::Select(..)
        | InstKind::SiToFp(_)
        | InstKind::FpToSi(_)
        | InstKind::FpToInt32(_)
        | InstKind::ZExtBoolToI64(_)
        | InstKind::ZExtI32ToI64(_)
        | InstKind::BitCastF64ToI64(_)
        | InstKind::BitCastI64ToF64(_)
        | InstKind::TruncI64ToBool(_)
        | InstKind::Neg(_)
        | InstKind::Ctpop(_)
        | InstKind::Cttz(_) => true,
        _ => false,
    }
}

fn plan_loop(func: &Function, la: &LoopAnalysis, l: &NaturalLoop, uses: &Uses) -> Option<Plan> {
    let h = l.header;
    if la
        .loops()
        .iter()
        .any(|o| o.header != h && l.contains(&o.header))
    {
        return None;
    }
    let [latch] = *l.back_edge_sources.as_slice() else {
        return None;
    };
    if latch == h || !matches!(blk(func, latch).term, Terminator::Br(t) if t == h) {
        return None;
    }
    let Terminator::CondBr {
        cond: Operand::Value(_),
        then_blk,
        else_blk,
    } = blk(func, h).term
    else {
        return None;
    };
    let b0 = match (l.contains(&then_blk), l.contains(&else_blk)) {
        (true, false) => then_blk,
        (false, true) => else_blk,
        _ => return None,
    };
    // the header is entered from the latch and one block outside
    let mut hp = uses.preds.get(&h)?.clone();
    hp.sort_by_key(|b| b.0);
    if hp.len() != 2 || !hp.contains(&latch) || hp.iter().any(|p| *p != latch && l.contains(p)) {
        return None;
    }
    let preheader = *hp.iter().find(|p| **p != latch)?;
    let chain = chain(func, uses, l, h, b0, latch)?;
    let body: Vec<BlockId> = std::iter::once(h).chain(chain.iter().copied()).collect();
    let insts: usize = body.iter().map(|b| blk(func, *b).insts.len()).sum();
    if insts > MAX_INSTS {
        return None;
    }
    let cells = uses.cells(func);
    for &b in &body {
        for inst in &blk(func, b).insts {
            let r = inst.result?;
            if !pure(&inst.kind) {
                return None;
            }
            // the clone renames every value that is not a cell: one
            // defined twice would be read under its old id past a clone
            if !cells.contains(&r) && uses.defs.get(&r).map(Vec::len) != Some(1) {
                return None;
            }
            // a header cell would be redefined by the clone's header
            // before the first copy's latch reads it
            if b == h && cells.contains(&r) {
                return None;
            }
            // the second copy defines its own ids: nothing but a cell
            // may be read after the loop
            if !cells.contains(&r)
                && uses
                    .reads
                    .get(&r)
                    .is_some_and(|rs| rs.iter().any(|(rb, _)| !l.contains(rb)))
            {
                return None;
            }
        }
    }
    let (site, fdiv_at, fptosi_at, yf, q, t, iv, inv, inv_is_lhs) = quotient(func, uses, l, &body)?;
    let (step_add, step, step_at) = step(func, uses, l, latch, iv)?;
    // the counter's step comes after the quotient in the iteration
    let pos = |b: BlockId| {
        body.iter()
            .position(|x| *x == b)
            .expect("block of the loop")
    };
    if !cells.contains(&iv) || (pos(step_at.0), step_at.1) <= (pos(site), fdiv_at) {
        return None;
    }
    Some(Plan {
        header: h,
        preheader,
        float_counter: false,
        chain,
        iv,
        step,
        step_add,
        step_at,
        site,
        fdiv_at,
        fptosi_at,
        yf,
        q,
        t,
        inv,
        inv_is_lhs,
        cells,
    })
}

/// The body as one straight run of blocks from `b0` to the latch; each
/// block leaves the loop on at most one edge.
fn chain(
    func: &Function,
    uses: &Uses,
    l: &NaturalLoop,
    h: BlockId,
    b0: BlockId,
    latch: BlockId,
) -> Option<Vec<BlockId>> {
    let mut chain = Vec::new();
    let (mut prev, mut cur) = (h, b0);
    while cur != h {
        if chain.len() >= MAX_CHAIN_BLOCKS
            || !l.contains(&cur)
            || uses.preds.get(&cur).map(Vec::as_slice) != Some(&[prev])
        {
            return None;
        }
        let next = match blk(func, cur).term {
            Terminator::Br(n) => n,
            Terminator::CondBr {
                cond: Operand::Value(_),
                then_blk,
                else_blk,
            } => match (l.contains(&then_blk), l.contains(&else_blk)) {
                (true, false) => then_blk,
                (false, true) => else_blk,
                _ => return None,
            },
            _ => return None,
        };
        chain.push(cur);
        (prev, cur) = (cur, next);
    }
    (chain.last() == Some(&latch) && l.body.len() == chain.len() + 1).then_some(chain)
}

type Quotient = (
    BlockId,
    usize,
    usize,
    ValueId,
    ValueId,
    ValueId,
    ValueId,
    Operand,
    bool,
);

/// The loop's one `fdiv`: a loop-invariant value against `sitofp iv`
/// from the same block, read once by an `fptosi` of the same block.
fn quotient(func: &Function, uses: &Uses, l: &NaturalLoop, body: &[BlockId]) -> Option<Quotient> {
    let mut found = None;
    for &b in body {
        for (i, inst) in blk(func, b).insts.iter().enumerate() {
            if let InstKind::BinOp(BinOp::FDiv, a, c) = inst.kind {
                if found.is_some() {
                    return None;
                }
                found = Some((b, i, a, c));
            }
        }
    }
    let (site, fdiv_at, a, c) = found?;
    let insts = &blk(func, site).insts;
    let q = insts[fdiv_at].result?;
    let invariant = |op: &Operand| match op {
        Operand::Value(v) => {
            func.value_type(*v) == Type::F64
                && uses
                    .defs
                    .get(v)
                    .is_some_and(|ds| ds.iter().all(|(db, _)| !l.contains(db)))
        }
        _ => false,
    };
    // `sitofp iv` defined once, earlier in the site block
    let converted = |op: &Operand| -> Option<(ValueId, ValueId)> {
        let Operand::Value(yf) = op else {
            return None;
        };
        let (db, di) = uses.single_def(*yf)?;
        if db != site || di >= fdiv_at {
            return None;
        }
        match insts[di].kind {
            InstKind::SiToFp(Operand::Value(iv)) if func.value_type(iv) == Type::I64 => {
                Some((*yf, iv))
            }
            _ => None,
        }
    };
    let (yf, iv, inv, inv_is_lhs) = match (converted(&a), converted(&c)) {
        (Some((yf, iv)), None) if invariant(&c) => (yf, iv, c, false),
        (None, Some((yf, iv))) if invariant(&a) => (yf, iv, a, true),
        _ => return None,
    };
    // the quotient's one reader converts it back in the same block
    let [(rb, fptosi_at)] = *uses.reads.get(&q)?.as_slice() else {
        return None;
    };
    if rb != site || fptosi_at <= fdiv_at || fptosi_at >= insts.len() {
        return None;
    }
    let t = match insts[fptosi_at].kind {
        InstKind::FpToSi(Operand::Value(v)) if v == q => insts[fptosi_at].result?,
        _ => return None,
    };
    if uses.single_def(q).is_none() || uses.single_def(t).is_none() {
        return None;
    }
    Some((site, fdiv_at, fptosi_at, yf, q, t, iv, inv, inv_is_lhs))
}

/// `iv`'s only def in the loop is the latch's copy of `iv + C`, with
/// `C` a constant; answers the add's value, `C` and the add's place.
fn step(
    func: &Function,
    uses: &Uses,
    l: &NaturalLoop,
    latch: BlockId,
    iv: ValueId,
) -> Option<(ValueId, i64, (BlockId, usize))> {
    let in_loop: Vec<(BlockId, usize)> = uses
        .defs
        .get(&iv)?
        .iter()
        .copied()
        .filter(|(b, _)| l.contains(b))
        .collect();
    let [(b, i)] = in_loop[..] else {
        return None;
    };
    if b != latch {
        return None;
    }
    let InstKind::Copy(Type::I64, Operand::Value(s)) = blk(func, b).insts[i].kind else {
        return None;
    };
    let (sb, si) = uses.single_def(s)?;
    if !l.contains(&sb) {
        return None;
    }
    let c = match blk(func, sb).insts[si].kind {
        InstKind::BinOp(BinOp::Add, Operand::Value(x), Operand::ConstI64(c))
        | InstKind::BinOp(BinOp::Add, Operand::ConstI64(c), Operand::Value(x))
            if x == iv && (1..=MAX_STEP).contains(&c) =>
        {
            c
        }
        _ => return None,
    };
    Some((s, c, (sb, si)))
}
