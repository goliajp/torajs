//! Which outer loops the rewrite takes, and the regions it names.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::{LoopAnalysis, NaturalLoop};
use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Operand, Terminator, Type, ValueId,
    visit_value_operands,
};

use super::chain::{inner_chain, inner_exit, pre_chain};
use super::fit::{lane_count, regions_fit};
use super::flow::{escaping, lane_cells, lane_shared};
use super::retire::exit_cells;
use super::{MAX_STEP, MIN_LANES, Plan};

pub(super) struct Ctx {
    pub preds: HashMap<BlockId, Vec<BlockId>>,
    /// Every def site of each value.
    pub defs: HashMap<ValueId, Vec<(BlockId, usize)>>,
    pub uses: HashMap<ValueId, u32>,
}

pub(super) fn find_plan(func: &Function, done: &HashSet<BlockId>) -> Option<Plan> {
    let dom = DominatorTree::compute(func);
    let la = LoopAnalysis::compute(func, &dom);
    let ctx = context(func);
    la.loops()
        .iter()
        .filter(|l| !done.contains(&l.header))
        .find_map(|l| plan_loop(func, &la, l, &ctx))
}

fn context(func: &Function) -> Ctx {
    let mut ctx = Ctx {
        preds: HashMap::default(),
        defs: HashMap::default(),
        uses: HashMap::default(),
    };
    for block in &func.blocks {
        for s in successors(&block.term) {
            ctx.preds.entry(s).or_default().push(block.id);
        }
        for (i, inst) in block.insts.iter().enumerate() {
            if let Some(r) = inst.result {
                ctx.defs.entry(r).or_default().push((block.id, i));
            }
            visit_value_operands(&inst.kind, |v| *ctx.uses.entry(v).or_insert(0) += 1);
        }
        if let Some(v) = term_value(&block.term) {
            *ctx.uses.entry(v).or_insert(0) += 1;
        }
    }
    ctx
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

pub(super) fn term_value(term: &Terminator) -> Option<ValueId> {
    match term {
        Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) => Some(*v),
        _ => None,
    }
}

fn plan_loop(func: &Function, la: &LoopAnalysis, l: &NaturalLoop, ctx: &Ctx) -> Option<Plan> {
    let h = l.header;
    let [latch] = *l.back_edge_sources.as_slice() else {
        return None;
    };
    if latch == h || !matches!(blk(func, latch).term, Terminator::Br(t) if t == h) {
        return None;
    }
    let body: HashSet<BlockId> = l.body.iter().copied().collect();
    let Terminator::CondBr {
        cond: Operand::Value(c),
        then_blk,
        else_blk,
    } = blk(func, h).term
    else {
        return None;
    };
    let (p0, on_true) = match (body.contains(&then_blk), body.contains(&else_blk)) {
        (true, false) => (then_blk, true),
        (false, true) => (else_blk, false),
        _ => return None,
    };
    let (iv, pred, bound) = counted_test(func, h, c, on_true, ctx)?;
    // PH, ahead of the loop, computes `bound - (lanes-1)·C`
    if let Operand::Value(b) = bound
        && ctx
            .defs
            .get(&b)
            .is_some_and(|ds| ds.iter().any(|(d, _)| body.contains(d)))
    {
        return None;
    }
    let [inner] = la
        .loops()
        .iter()
        .filter(|o| o.header != h && body.contains(&o.header))
        .collect::<Vec<_>>()[..]
    else {
        return None;
    };
    let ih = inner.header;
    let (pre_blocks, ih_entry) = pre_chain(func, ctx, &body, h, p0, ih)?;
    let [il] = *inner.back_edge_sources.as_slice() else {
        return None;
    };
    let mut ih_preds = ctx.preds.get(&ih)?.clone();
    ih_preds.sort_by_key(|b| b.0);
    let mut want = vec![ih_entry, il];
    want.sort_by_key(|b| b.0);
    if ih_preds != want {
        return None;
    }
    let (inner_on_true, cont, px) = inner_exit(func, inner, ih)?;
    let (chain, inner_exits) = inner_chain(func, ctx, &body, inner, ih, cont)?;
    let mut fixed: HashSet<BlockId> = pre_blocks.iter().copied().collect();
    fixed.insert(h);
    if fixed.contains(&px) || !body.contains(&px) || inner_exits.values().any(|x| fixed.contains(x))
    {
        return None;
    }
    let mut post: Vec<BlockId> = body
        .iter()
        .copied()
        .filter(|b| !fixed.contains(b) && !inner.contains(b))
        .collect();
    post.sort_by_key(|b| b.0);
    let post_set: HashSet<BlockId> = post.iter().copied().collect();
    for &b in &post {
        for s in successors(&blk(func, b).term) {
            let ok = if s == h {
                b == latch
            } else {
                !body.contains(&s) || post_set.contains(&s)
            };
            if !ok {
                return None;
            }
        }
    }
    let step = iv_step(func, ctx, &body, latch, iv)?;
    let plan = Plan {
        header: h,
        pre_blocks,
        skip: (ctx.uses.get(&c) == Some(&1)).then_some(c),
        iv,
        step,
        lanes: MIN_LANES,
        pred,
        bound,
        inner_header: ih,
        inner_on_true,
        chain,
        masked: !inner_exits.is_empty(),
        exit_cells: None,
        post,
        body,
        lane_cells: Vec::new(),
        escaping: HashSet::default(),
        shared: HashSet::default(),
    };
    complete(func, ctx, plan)
}

/// The plan with what its lanes keep, leave and share, once its regions
/// fit; `None` where they do not, or where the counter may wrap.
fn complete(func: &Function, ctx: &Ctx, mut plan: Plan) -> Option<Plan> {
    if !regions_fit(func, &plan) {
        return None;
    }
    plan.lane_cells = lane_cells(func, ctx, &plan)?;
    plan.escaping = escaping(func, ctx, &plan)?;
    plan.shared = lane_shared(func, &plan);
    plan.exit_cells = plan.masked.then(|| exit_cells(func, &plan)).flatten();
    plan.lanes = lane_count(func, &plan);
    wrap_ok(&plan).then_some(plan)
}

pub(super) fn blk(func: &Function, b: BlockId) -> &torajs_core::ssa::Block {
    &func.blocks[b.0 as usize]
}

/// The header's exit test as `iv <pred> bound` with `pred` `Slt`/`Sle`.
fn counted_test(
    func: &Function,
    h: BlockId,
    c: ValueId,
    on_true: bool,
    ctx: &Ctx,
) -> Option<(ValueId, IPred, Operand)> {
    let [(db, di)] = *ctx.defs.get(&c)?.as_slice() else {
        return None;
    };
    if db != h {
        return None;
    }
    let InstKind::ICmp(p, a, b) = blk(func, h).insts[di].kind else {
        return None;
    };
    let p = if on_true {
        p
    } else {
        match p {
            IPred::Slt => IPred::Sge,
            IPred::Sge => IPred::Slt,
            IPred::Sle => IPred::Sgt,
            IPred::Sgt => IPred::Sle,
            _ => return None,
        }
    };
    let (iv, bound, pred) = match p {
        IPred::Slt | IPred::Sle => (a, b, p),
        IPred::Sgt => (b, a, IPred::Slt),
        IPred::Sge => (b, a, IPred::Sle),
        _ => return None,
    };
    let Operand::Value(iv) = iv else {
        return None;
    };
    if func.value_type(iv) != Type::I64 {
        return None;
    }
    match bound {
        Operand::ConstI64(_) => {}
        Operand::Value(v) if func.value_type(v) == Type::I64 => {}
        _ => return None,
    }
    Some((iv, pred, bound))
}

/// `iv`'s only def inside the loop is the latch's copy of `iv + C`
/// (or that add itself).
fn iv_step(
    func: &Function,
    ctx: &Ctx,
    body: &HashSet<BlockId>,
    latch: BlockId,
    iv: ValueId,
) -> Option<i64> {
    let in_loop: Vec<(BlockId, usize)> = ctx
        .defs
        .get(&iv)?
        .iter()
        .copied()
        .filter(|(b, _)| body.contains(b))
        .collect();
    let [(b, i)] = in_loop[..] else {
        return None;
    };
    if b != latch {
        return None;
    }
    let add_of_iv = |kind: &InstKind| match *kind {
        InstKind::BinOp(BinOp::Add, Operand::Value(x), Operand::ConstI64(c))
        | InstKind::BinOp(BinOp::Add, Operand::ConstI64(c), Operand::Value(x))
            if x == iv && (1..=MAX_STEP).contains(&c) =>
        {
            Some(c)
        }
        _ => None,
    };
    let kind = &blk(func, b).insts[i].kind;
    if let InstKind::Copy(_, Operand::Value(x)) = kind {
        // the step may sit anywhere in the body (an overflow guard
        // between it and the latch); iv has no other def to read
        let [(xb, xi)] = *ctx.defs.get(x)?.as_slice() else {
            return None;
        };
        if !body.contains(&xb) || (xb == latch && xi > i) {
            return None;
        }
        return add_of_iv(&blk(func, xb).insts[xi].kind);
    }
    add_of_iv(kind)
}

fn wrap_ok(p: &Plan) -> bool {
    let span = p.step * (p.lanes as i64 - 1);
    match p.bound {
        Operand::ConstI64(n) => n.checked_sub(span).is_some(),
        _ => true,
    }
}
