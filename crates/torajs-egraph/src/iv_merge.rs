//! Induction-variable merging — two counters of one loop that are set
//! in the same block and stepped by the same constant in the same block
//! differ by a fixed amount `d` everywhere outside the two short windows
//! where one has moved and the other not yet, so one of them can be
//! read as the other plus `d` (textbook induction-variable elimination).
//!
//! The counter that goes is one whose reads inside the loop are all
//! scaled offsets of memory accesses through a base computed before the
//! loop: `access(base, j << 3)` with `j = i + d` is
//! `access(base + (d << 3), i << 3)`, and `base + (d << 3)` is computed
//! once, beside the two initial copies. A read after the loop takes
//! `i + d`. Its step dies with it, so the loop carries one counter.
//!
//! The pre-reserved fill loop is the shape this is for: the running
//! length addresses the slot, the program's own counter is what gets
//! stored and compared, and both step by one.

use std::collections::HashMap;

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;
use torajs_core::ssa::{
    BinOp, BlockId, Function, Inst, InstKind, Module, Operand, Type, ValueId, ValueInfo,
    visit_value_operands,
};

use crate::float_demote::cfg::term_operand_value;

#[derive(Debug, Default)]
pub struct IvMergeStats {
    pub merged: usize,
}

pub fn merge_ivs(module: &mut Module) -> IvMergeStats {
    let mut stats = IvMergeStats::default();
    for func in &mut module.funcs {
        // one merge at a time: each rewrite moves instructions, so the
        // positions the next one is planned from are read afresh
        while let Some(plan) = find_merge(func) {
            apply(func, plan);
            stats.merged += 1;
        }
    }
    stats
}

/// A counter cell: `c = copy init` at `init_at`, `t = c + k` then
/// `c = copy t` in one block.
#[derive(Clone, Copy)]
struct Counter {
    cell: ValueId,
    init: Operand,
    init_at: (usize, usize),
    step_add: (usize, usize),
    step_at: (usize, usize),
    k: i64,
}

/// `gone` is read as `kept + d` from here on.
struct Plan {
    kept: Counter,
    gone: Counter,
    /// Where `d` and the advanced bases go: just after both inits.
    insert_at: (usize, usize),
    /// `(shift inst, access inst)` pairs inside the loop.
    offsets: Vec<((usize, usize), (usize, usize))>,
    /// Instructions after the loop that read `gone`.
    reads_after: Vec<(usize, usize)>,
}

fn find_merge(func: &Function) -> Option<Plan> {
    let mut defs: HashMap<ValueId, Vec<(usize, usize)>> = HashMap::new();
    let mut uses: HashMap<ValueId, Vec<(usize, usize)>> = HashMap::new();
    let mut term_uses: HashMap<ValueId, usize> = HashMap::new();
    for (bi, block) in func.blocks.iter().enumerate() {
        for (ii, inst) in block.insts.iter().enumerate() {
            if let Some(r) = inst.result {
                defs.entry(r).or_default().push((bi, ii));
            }
            visit_value_operands(&inst.kind, |v| {
                let at = uses.entry(v).or_default();
                // an instruction reading a value twice is one reader
                if at.last() != Some(&(bi, ii)) {
                    at.push((bi, ii));
                }
            });
        }
        if let Some(v) = term_operand_value(&block.term) {
            *term_uses.entry(v).or_insert(0) += 1;
        }
    }
    let dom = DominatorTree::compute(func);
    let counters: Vec<Counter> = defs
        .iter()
        .filter_map(|(c, ds)| counter_of(func, *c, ds, &defs, &dom))
        .collect();
    if counters.len() < 2 {
        return None;
    }
    let loops = LoopAnalysis::compute(func, &dom);
    let mut sorted = counters;
    sorted.sort_by_key(|c| c.cell.0);
    for gone in &sorted {
        for kept in &sorted {
            let same_shape = kept.cell != gone.cell
                && kept.k == gone.k
                && kept.init_at.0 == gone.init_at.0
                && kept.step_at.0 == gone.step_at.0;
            if !same_shape {
                continue;
            }
            // the innermost loop around the step that the inits sit above
            let Some(body) = loops
                .loops()
                .iter()
                .filter(|l| l.body.contains(&BlockId(gone.step_at.0 as u32)))
                .min_by_key(|l| l.body.len())
                .filter(|l| {
                    let init = BlockId(gone.init_at.0 as u32);
                    !l.body.contains(&init) && dom.dominates(init, l.header)
                })
                .map(|l| &l.body)
            else {
                continue;
            };
            let reads = Reads {
                defs: &defs,
                uses: &uses,
                term_uses: &term_uses,
            };
            if let Some(plan) = plan_merge(func, *kept, *gone, body, &dom, &reads) {
                return Some(plan);
            }
        }
    }
    None
}

fn counter_of(
    func: &Function,
    cell: ValueId,
    ds: &[(usize, usize)],
    defs: &HashMap<ValueId, Vec<(usize, usize)>>,
    dom: &DominatorTree,
) -> Option<Counter> {
    if ds.len() != 2 || func.values[cell.0 as usize].ty != Type::I64 {
        return None;
    }
    let kind = |at: (usize, usize)| &func.blocks[at.0].insts[at.1].kind;
    let step_of = |at: (usize, usize)| -> Option<((usize, usize), i64)> {
        let InstKind::Copy(_, Operand::Value(t)) = kind(at) else {
            return None;
        };
        let [t_at] = defs.get(t)?.as_slice() else {
            return None;
        };
        // the sum is formed on every way to the copy (a `for` loop's
        // step block takes it from the body); that it is formed inside
        // the loop is checked once the loop is known
        let before = if t_at.0 == at.0 {
            t_at.1 < at.1
        } else {
            dom.dominates(BlockId(t_at.0 as u32), BlockId(at.0 as u32))
        };
        if !before {
            return None;
        }
        match kind(*t_at) {
            InstKind::BinOp(BinOp::Add, Operand::Value(v), Operand::ConstI64(k))
            | InstKind::BinOp(BinOp::Add, Operand::ConstI64(k), Operand::Value(v))
                if *v == cell =>
            {
                Some((*t_at, *k))
            }
            _ => None,
        }
    };
    let (init_at, (step_add, k), step_at) = match (step_of(ds[0]), step_of(ds[1])) {
        (None, Some(s)) => (ds[0], s, ds[1]),
        (Some(s), None) => (ds[1], s, ds[0]),
        _ => return None,
    };
    let InstKind::Copy(_, init) = kind(init_at) else {
        return None;
    };
    // the init is read again after the copy, when `d` is formed; a
    // parameter has no def and never changes
    let stable = match init {
        Operand::Value(v) => defs.get(v).is_none_or(|d| d.len() == 1) && *v != cell,
        Operand::ConstI64(_) => true,
        _ => false,
    };
    stable.then_some(Counter {
        cell,
        init: *init,
        init_at,
        step_add,
        step_at,
        k,
    })
}

struct Reads<'a> {
    defs: &'a HashMap<ValueId, Vec<(usize, usize)>>,
    uses: &'a HashMap<ValueId, Vec<(usize, usize)>>,
    term_uses: &'a HashMap<ValueId, usize>,
}

/// Whether `gone` can be read as `kept + d`, and how.
fn plan_merge(
    func: &Function,
    kept: Counter,
    gone: Counter,
    body: &[BlockId],
    dom: &DominatorTree,
    reads: &Reads,
) -> Option<Plan> {
    let Reads {
        defs,
        uses,
        term_uses,
    } = reads;
    let in_loop = |b: usize| body.contains(&BlockId(b as u32));
    if !in_loop(kept.step_add.0) || !in_loop(gone.step_add.0) {
        return None;
    }
    let inst = |at: (usize, usize)| &func.blocks[at.0].insts[at.1];
    let use_list = |v: ValueId| uses.get(&v).map(Vec::as_slice).unwrap_or(&[]);
    // the windows where one counter has moved and the other not yet
    let windows = [(kept.init_at, gone.init_at), (kept.step_at, gone.step_at)];
    for (a, b) in windows {
        let (lo, hi) = (a.1.min(b.1), a.1.max(b.1));
        let inside = |at: &(usize, usize)| at.0 == a.0 && at.1 > lo && at.1 < hi;
        if use_list(kept.cell)
            .iter()
            .chain(use_list(gone.cell))
            .any(inside)
        {
            return None;
        }
    }
    let insert_at = (kept.init_at.0, kept.init_at.1.max(gone.init_at.1) + 1);
    let available = |v: ValueId| match defs.get(&v).map(Vec::as_slice) {
        None => func.params.contains(&v),
        Some([(b, i)]) if *b == insert_at.0 => *i < insert_at.1,
        Some([(b, _)]) => dom.dominates(BlockId(*b as u32), BlockId(insert_at.0 as u32)),
        Some(_) => false,
    };
    let step_add = gone.step_add;
    let step_result = inst(step_add).result?;
    let read_by_a_branch = |v: ValueId| term_uses.contains_key(&v);
    if use_list(step_result).len() != 1 || read_by_a_branch(step_result) {
        return None;
    }
    if read_by_a_branch(gone.cell) {
        return None;
    }
    let mut offsets = Vec::new();
    let mut reads_after = Vec::new();
    for &at in use_list(gone.cell) {
        if at == step_add {
            continue;
        }
        if in_loop(at.0) {
            offsets.push((at, scaled_access(func, at, gone.cell, use_list, &in_loop)?));
        } else {
            let after_inits = at.0 != insert_at.0 || at.1 >= insert_at.1;
            if !(after_inits && dom.dominates(BlockId(insert_at.0 as u32), BlockId(at.0 as u32))) {
                return None;
            }
            reads_after.push(at);
        }
    }
    for (_, access) in &offsets {
        let base = match &inst(*access).kind {
            InstKind::LoadDyn(_, b, _) | InstKind::StoreDyn(_, b, _) => *b,
            _ => return None,
        };
        let Operand::Value(base) = base else {
            return None;
        };
        if !available(base) {
            return None;
        }
    }
    Some(Plan {
        kept,
        gone,
        insert_at,
        offsets,
        reads_after,
    })
}

/// `at` is `gone << 3` (or `gone * 8`) whose one reader is the offset of
/// a dynamic access in the loop; answers that access.
fn scaled_access<'a>(
    func: &Function,
    at: (usize, usize),
    gone: ValueId,
    use_list: impl Fn(ValueId) -> &'a [(usize, usize)],
    in_loop: &impl Fn(usize) -> bool,
) -> Option<(usize, usize)> {
    let inst = &func.blocks[at.0].insts[at.1];
    let scaled = matches!(
        inst.kind,
        InstKind::BinOp(BinOp::Shl, Operand::Value(v), Operand::ConstI64(3))
            | InstKind::BinOp(BinOp::Mul, Operand::Value(v), Operand::ConstI64(8))
            if v == gone
    );
    let m = inst.result.filter(|_| scaled)?;
    let [access] = use_list(m) else {
        return None;
    };
    let reads_as_offset = match &func.blocks[access.0].insts[access.1].kind {
        InstKind::LoadDyn(_, b, off) => *off == Operand::Value(m) && *b != Operand::Value(m),
        InstKind::StoreDyn(v, b, off) => {
            *off == Operand::Value(m) && *b != Operand::Value(m) && *v != Operand::Value(m)
        }
        _ => false,
    };
    (reads_as_offset && in_loop(access.0)).then_some(*access)
}

fn apply(func: &mut Function, plan: Plan) {
    let Plan {
        kept,
        gone,
        insert_at,
        offsets,
        reads_after,
    } = plan;
    let new_value = |func: &mut Function, ty: Type| {
        let v = ValueId(func.values.len() as u32);
        func.values.push(ValueInfo { ty, name: None });
        v
    };
    let inst = |result: Option<ValueId>, kind: InstKind| Inst {
        result,
        kind,
        origin: None,
    };
    // `d = gone.init - kept.init`, then one advanced copy per base
    let mut head: Vec<Inst> = Vec::new();
    let d = match (gone.init, kept.init) {
        (g, Operand::ConstI64(0)) => g,
        (Operand::ConstI64(g), Operand::ConstI64(k)) => Operand::ConstI64(g.wrapping_sub(k)),
        (g, k) => {
            let v = new_value(func, Type::I64);
            head.push(inst(Some(v), InstKind::BinOp(BinOp::Sub, g, k)));
            Operand::Value(v)
        }
    };
    let d8 = match d {
        Operand::ConstI64(n) => Operand::ConstI64(n.wrapping_mul(8)),
        _ => {
            let v = new_value(func, Type::I64);
            head.push(inst(
                Some(v),
                InstKind::BinOp(BinOp::Shl, d, Operand::ConstI64(3)),
            ));
            Operand::Value(v)
        }
    };
    let mut advanced: HashMap<ValueId, ValueId> = HashMap::new();
    for (shift, access) in &offsets {
        let a = &mut func.blocks[access.0].insts[access.1].kind;
        let (InstKind::LoadDyn(_, b, _) | InstKind::StoreDyn(_, b, _)) = a else {
            unreachable!("planned as a dynamic access")
        };
        let Operand::Value(base) = *b else {
            unreachable!("planned with a value base")
        };
        if d8 != Operand::ConstI64(0) {
            let next = match advanced.get(&base) {
                Some(n) => *n,
                None => {
                    let ty = func.values[base.0 as usize].ty;
                    let n = new_value(func, ty);
                    let kind = InstKind::BinOp(BinOp::Add, Operand::Value(base), d8);
                    head.push(inst(Some(n), kind));
                    advanced.insert(base, n);
                    n
                }
            };
            let a = &mut func.blocks[access.0].insts[access.1].kind;
            if let InstKind::LoadDyn(_, b, _) | InstKind::StoreDyn(_, b, _) = a {
                *b = Operand::Value(next);
            }
        }
        if let InstKind::BinOp(_, x, _) = &mut func.blocks[shift.0].insts[shift.1].kind {
            *x = Operand::Value(kept.cell);
        }
    }
    // reads after the loop take `kept + d`, each just before itself
    let mut before: HashMap<(usize, usize), Inst> = HashMap::new();
    for at in &reads_after {
        let read = match d {
            Operand::ConstI64(0) => kept.cell,
            _ => {
                let v = new_value(func, Type::I64);
                let kind = InstKind::BinOp(BinOp::Add, Operand::Value(kept.cell), d);
                before.insert(*at, inst(Some(v), kind));
                v
            }
        };
        torajs_core::ssa::rewrite_value_operands(&mut func.blocks[at.0].insts[at.1].kind, |v| {
            if v == gone.cell { read } else { v }
        });
    }
    let dropped = [gone.init_at, gone.step_add, gone.step_at];
    for bi in 0..func.blocks.len() {
        let old = std::mem::take(&mut func.blocks[bi].insts);
        let mut out = Vec::with_capacity(old.len() + head.len());
        for (ii, i) in old.into_iter().enumerate() {
            if (bi, ii) == insert_at {
                out.append(&mut head);
            }
            if let Some(pre) = before.remove(&(bi, ii)) {
                out.push(pre);
            }
            if !dropped.contains(&(bi, ii)) {
                out.push(i);
            }
        }
        if bi == insert_at.0 {
            out.append(&mut head);
        }
        func.blocks[bi].insts = out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, IPred, Terminator};

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn void(kind: InstKind) -> Inst {
        Inst {
            result: None,
            kind,
            origin: None,
        }
    }

    /// The fill loop: `%3` the program's counter, `%4` the running
    /// length addressing `%0`, both from `%1` trips.
    ///
    /// ```text
    /// bb0: %3 = copy 0 ; %4 = copy %2 ; br bb1
    /// bb1: %5 = icmp slt %3, %1 ; cond_br %5, bb2, bb3
    /// bb2: %6 = shl %4, 3 ; store_dyn %3, %0 + %6
    ///      %7 = add %4, <len_step> ; %8 = add %3, 1
    ///      %3 = copy %8 ; <between> ; %4 = copy %7 ; br bb1
    /// bb3: %9 = copy %4 ; ret %9
    /// ```
    fn fill_loop(len_step: i64, between: Vec<Inst>) -> Function {
        let mut body = vec![
            inst(6, InstKind::BinOp(BinOp::Shl, v(4), Operand::ConstI64(3))),
            void(InstKind::StoreDyn(v(3), v(0), v(6))),
            inst(
                7,
                InstKind::BinOp(BinOp::Add, v(4), Operand::ConstI64(len_step)),
            ),
            inst(8, InstKind::BinOp(BinOp::Add, v(3), Operand::ConstI64(1))),
            inst(3, InstKind::Copy(Type::I64, v(8))),
        ];
        body.extend(between);
        body.push(inst(4, InstKind::Copy(Type::I64, v(7))));
        let block = |id: u32, insts: Vec<Inst>, term: Terminator| Block {
            id: BlockId(id),
            insts,
            term,
        };
        let entry = vec![
            inst(3, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
            inst(4, InstKind::Copy(Type::I64, v(2))),
        ];
        let test = Terminator::CondBr {
            cond: v(5),
            then_blk: BlockId(2),
            else_blk: BlockId(3),
        };
        let exit = vec![inst(9, InstKind::Copy(Type::I64, v(4)))];
        Function {
            name: "f".into(),
            params: vec![ValueId(0), ValueId(1), ValueId(2)],
            ret: Type::I64,
            blocks: vec![
                block(0, entry, Terminator::Br(BlockId(1))),
                block(
                    1,
                    vec![inst(5, InstKind::ICmp(IPred::Slt, v(3), v(1)))],
                    test,
                ),
                block(2, body, Terminator::Br(BlockId(1))),
                block(3, exit, Terminator::Ret(Some(v(9)))),
            ],
            values: (0..11)
                .map(|i| ValueInfo {
                    ty: match i {
                        0 => Type::Ptr,
                        5 => Type::Bool,
                        _ => Type::I64,
                    },
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    fn run(f: Function) -> (Function, usize) {
        let mut m = Module {
            funcs: vec![f],
            ..Default::default()
        };
        let stats = merge_ivs(&mut m);
        (m.funcs.remove(0), stats.merged)
    }

    #[test]
    fn the_length_becomes_the_counter_plus_its_start() {
        let (f, merged) = run(fill_loop(1, vec![]));
        assert_eq!(merged, 1);
        let all = || f.blocks.iter().flat_map(|b| &b.insts);
        assert!(
            !all().any(|i| i.result == Some(ValueId(4))),
            "the length cell is gone"
        );
        // the base moves by the start, once, before the loop
        let entry = &f.blocks[0].insts;
        let InstKind::BinOp(BinOp::Shl, start, _) = entry[1].kind else {
            panic!("the start is scaled in the entry: {entry:?}");
        };
        assert_eq!(start, v(2));
        let advanced = entry[2].result.unwrap();
        assert_eq!(
            entry[2].kind,
            InstKind::BinOp(BinOp::Add, v(0), Operand::Value(entry[1].result.unwrap()))
        );
        let body = &f.blocks[2].insts;
        assert_eq!(
            body[0].kind,
            InstKind::BinOp(BinOp::Shl, v(3), Operand::ConstI64(3))
        );
        assert_eq!(
            body[1].kind,
            InstKind::StoreDyn(v(3), Operand::Value(advanced), v(6))
        );
        // after the loop the length reads as counter + start
        let exit = &f.blocks[3].insts;
        assert_eq!(exit[0].kind, InstKind::BinOp(BinOp::Add, v(3), v(2)));
        assert_eq!(
            exit[1].kind,
            InstKind::Copy(Type::I64, Operand::Value(exit[0].result.unwrap()))
        );
    }

    #[test]
    fn different_steps_are_not_merged() {
        let (_, merged) = run(fill_loop(2, vec![]));
        assert_eq!(merged, 0);
    }

    #[test]
    fn a_read_between_the_two_steps_blocks_the_merge() {
        let read = inst(10, InstKind::BinOp(BinOp::Add, v(3), Operand::ConstI64(5)));
        let (_, merged) = run(fill_loop(1, vec![read]));
        assert_eq!(merged, 0);
    }

    #[test]
    fn a_length_the_loop_reads_otherwise_is_kept() {
        let mut f = fill_loop(1, vec![]);
        f.blocks[1].insts[0] = inst(5, InstKind::ICmp(IPred::Slt, v(4), v(1)));
        let (_, merged) = run(f);
        assert_eq!(merged, 0);
    }

    /// The `for` shape: the sums formed in the body, the copies in a
    /// step block of their own.
    fn split_step(mut f: Function) -> Function {
        let copies = f.blocks[2].insts.split_off(4);
        f.blocks[2].term = Terminator::Br(BlockId(4));
        f.blocks.push(Block {
            id: BlockId(4),
            insts: copies,
            term: Terminator::Br(BlockId(1)),
        });
        f
    }

    #[test]
    fn a_step_block_after_the_body_still_merges() {
        let (f, merged) = run(split_step(fill_loop(1, vec![])));
        assert_eq!(merged, 1);
        assert!(
            f.blocks[4]
                .insts
                .iter()
                .all(|i| i.result != Some(ValueId(4)))
        );
    }

    #[test]
    fn a_sum_formed_before_the_loop_is_not_a_step() {
        // `%7 = %4 + 1` once, above the loop: `%4` is then the same
        // value every trip, not a counter
        let mut f = split_step(fill_loop(1, vec![]));
        let sum = f.blocks[2].insts.remove(2);
        f.blocks[0].insts.push(sum);
        let (_, merged) = run(f);
        assert_eq!(merged, 0);
    }
}
