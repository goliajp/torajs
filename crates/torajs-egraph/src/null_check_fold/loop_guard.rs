//! Loop-invariant nullish guards, and the string header reads behind
//! them, out of loops.
//!
//! The inline guard in front of a member read is a chain of compares
//! of the receiver against null and the cells its slot family spells
//! `undefined` with, each branching to a raise block. Lowering settles
//! a binding after its first guard on a path, but a loop header is
//! reached on two paths, so `while (i < s.length)` tested `s` once per
//! iteration:
//!
//! ```text
//! head:  cond_br (icmp eq s, null), raise, n1
//! n1:    cond_br (icmp eq s, @STR_UNDEF), raise, n2 …
//! ```
//!
//! A test that opens the header, reads nothing the loop defines and
//! leaves through a raise block answers the same on every iteration:
//! if it fails, it fails on the first, before the loop has done
//! anything. It moves to a fresh block on the way in (the compare and
//! what it reads with it) and the header branches on unconditionally.
//! The next test of the chain then opens the header and follows.
//!
//! With the null test in front of the loop the receiver is a live
//! cell there, and what the loop reads from its header that no
//! instruction in the loop can change moves out too: a string's
//! length, and the encoding bit of its flags word (masked, so the
//! refcount in the same word does not matter). Both are written only
//! inside a runtime call handed the string, and the loop makes none.
//!
//! `TORAJS_LOOP_GUARD_OFF=1` skips, `TORAJS_LOOP_GUARD_STATS=1` counts.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::{LoopAnalysis, NaturalLoop};
use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Module, Operand, Terminator, Type, ValueId,
    rewrite_value_operands, visit_value_operands,
};

use crate::float_demote::cfg::multi_def_values;
use crate::optimize::classify_pure;
use crate::throw_check_fold::{probe_of, propagate_path_reads, successors};

/// `IS_LATIN1` in a string's packed header word (flags u16 at +6, bit 1).
const STR_IS_LATIN1: i64 = 0x0002 << 48;
/// Byte offset of a string's length word (an `I32`).
const STR_LEN_OFF: u64 = 8;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LoopGuardStats {
    /// Header tests moved in front of their loop.
    pub guards_hoisted: u32,
    /// String header reads moved in front of their loop, and the
    /// repeats of them folded onto the moved one.
    pub reads_hoisted: u32,
}

pub fn hoist_loop_guards(module: &mut Module) -> LoopGuardStats {
    let mut stats = LoopGuardStats::default();
    for func in module.funcs.iter_mut() {
        if !func.is_declaration() {
            hoist_fn(func, &mut stats);
        }
    }
    stats
}

fn loops_of(func: &Function) -> Vec<NaturalLoop> {
    let dom = DominatorTree::compute(func);
    LoopAnalysis::compute(func, &dom).loops().to_vec()
}

fn hoist_fn(func: &mut Function, stats: &mut LoopGuardStats) {
    let multi = multi_def_values(func);
    // per loop header: the receivers whose null test now sits in front
    let mut live: HashMap<BlockId, Vec<ValueId>> = HashMap::new();
    while let Some((lp, plan)) = loops_of(func)
        .into_iter()
        .find_map(|lp| plan_guard(func, &lp, &multi).map(|p| (lp, p)))
    {
        if let Some(v) = plan.null_of {
            live.entry(lp.header).or_default().push(v);
        }
        apply_guard(func, &lp, plan);
        stats.guards_hoisted += 1;
    }
    if live.is_empty() {
        return;
    }
    // widest loop first: a read moves as far out as its guard allows
    let mut loops = loops_of(func);
    loops.sort_by_key(|l| std::cmp::Reverse(l.body.len()));
    for lp in &loops {
        if let Some(values) = live.get(&lp.header) {
            // a plain preheader again, for the passes that look for one
            let pre = func.add_block();
            func.set_term(pre, Terminator::Br(lp.header));
            enter_through(func, lp, pre);
            for &v in values {
                stats.reads_hoisted += hoist_header_reads(func, lp, pre, v, &multi);
            }
        }
    }
}

/// One header test that can move in front of the loop.
struct Plan {
    /// Position of the block the test ends.
    tail: usize,
    /// Values the header segment defines that the test reads, to move
    /// along.
    cone: HashSet<ValueId>,
    /// Blocks of the header segment, in order.
    segment: Vec<usize>,
    cond: ValueId,
    raise: BlockId,
    stay: BlockId,
    raise_on_true: bool,
    /// The receiver, when this is its test against null.
    null_of: Option<ValueId>,
}

/// May sit in a header segment in front of the test: computes in
/// registers only.
fn register_only(kind: &InstKind) -> bool {
    classify_pure(kind) || matches!(kind, InstKind::GlobalRef(_) | InstKind::StaticStrRef(_))
}

fn plan_guard(func: &Function, lp: &NaturalLoop, multi: &HashSet<ValueId>) -> Option<Plan> {
    if lp.header.0 == 0 {
        return None;
    }
    let body: HashSet<usize> = lp.body.iter().map(|b| b.0 as usize).collect();
    let mut preds = vec![0u32; func.blocks.len()];
    for block in &func.blocks {
        for s in successors(&block.term) {
            preds[s.0 as usize] += 1;
        }
    }
    // the header and what it falls into with no other way in
    let mut segment = vec![lp.header.0 as usize];
    let (cond, then_blk, else_blk) = loop {
        let cur = *segment.last()?;
        if !func.blocks[cur]
            .insts
            .iter()
            .all(|i| register_only(&i.kind))
        {
            return None;
        }
        match &func.blocks[cur].term {
            Terminator::Br(n)
                if body.contains(&(n.0 as usize))
                    && preds[n.0 as usize] == 1
                    && !segment.contains(&(n.0 as usize)) =>
            {
                segment.push(n.0 as usize);
            }
            Terminator::CondBr {
                cond: Operand::Value(c),
                then_blk,
                else_blk,
            } => break (*c, *then_blk, *else_blk),
            _ => return None,
        }
    };
    let leaves = |b: BlockId| !body.contains(&(b.0 as usize));
    let (raise, stay, raise_on_true) = match (leaves(then_blk), leaves(else_blk)) {
        (true, false) => (then_blk, else_blk, true),
        (false, true) => (else_blk, then_blk, false),
        _ => return None,
    };
    // a raise block: the raiser's call, then the probe that propagates
    let raise_blk = &func.blocks[raise.0 as usize];
    if !matches!(raise_blk.insts.first()?.kind, InstKind::Call(..))
        || probe_of(func, raise.0 as usize).is_none()
        || !propagate_path_reads(func, &body, raise)?.is_empty()
    {
        return None;
    }
    // everything the test reads is computed by the segment from
    // values the loop does not define
    let in_segment: HashMap<ValueId, &InstKind> = segment
        .iter()
        .flat_map(|&b| &func.blocks[b].insts)
        .filter_map(|i| i.result.map(|r| (r, &i.kind)))
        .collect();
    let in_body: HashSet<ValueId> = body
        .iter()
        .flat_map(|&b| &func.blocks[b].insts)
        .filter_map(|i| i.result)
        .collect();
    let mut cone: HashSet<ValueId> = HashSet::new();
    let mut work = vec![cond];
    while let Some(v) = work.pop() {
        if multi.contains(&v) {
            return None;
        }
        match in_segment.get(&v) {
            Some(kind) => {
                if cone.insert(v) {
                    visit_value_operands(kind, |o| work.push(o));
                }
            }
            None if in_body.contains(&v) => return None,
            None => {}
        }
    }
    let null_of = func
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .find(|i| i.result == Some(cond))
        .and_then(|i| match &i.kind {
            InstKind::ICmp(IPred::Eq, Operand::Value(v), Operand::ConstPtrNull)
            | InstKind::ICmp(IPred::Eq, Operand::ConstPtrNull, Operand::Value(v))
                if raise_on_true =>
            {
                Some(*v)
            }
            _ => None,
        });
    Some(Plan {
        tail: *segment.last()?,
        cone,
        segment,
        cond,
        raise,
        stay,
        raise_on_true,
        null_of,
    })
}

/// Send every edge into `lp`'s header from outside the loop through
/// `through`.
fn enter_through(func: &mut Function, lp: &NaturalLoop, through: BlockId) {
    let header = lp.header;
    for block in func.blocks.iter_mut() {
        if block.id == through || lp.body.contains(&block.id) {
            continue;
        }
        match &mut block.term {
            Terminator::Br(t) if *t == header => *t = through,
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                for x in [then_blk, else_blk] {
                    if *x == header {
                        *x = through;
                    }
                }
            }
            _ => {}
        }
    }
}

fn apply_guard(func: &mut Function, lp: &NaturalLoop, plan: Plan) {
    let entry = func.add_block();
    let mut moved: Vec<Inst> = Vec::new();
    for &b in &plan.segment {
        let insts = std::mem::take(&mut func.blocks[b].insts);
        let (go, keep): (Vec<Inst>, Vec<Inst>) = insts
            .into_iter()
            .partition(|i| i.result.is_some_and(|r| plan.cone.contains(&r)));
        func.blocks[b].insts = keep;
        moved.extend(go);
    }
    func.blocks[entry.0 as usize].insts = moved;
    let (then_blk, else_blk) = if plan.raise_on_true {
        (plan.raise, lp.header)
    } else {
        (lp.header, plan.raise)
    };
    func.set_term(
        entry,
        Terminator::CondBr {
            cond: Operand::Value(plan.cond),
            then_blk,
            else_blk,
        },
    );
    enter_through(func, lp, entry);
    func.blocks[plan.tail].term = Terminator::Br(plan.stay);
}

/// Move the loop's reads of `v`'s length and encoding bit into `pre`;
/// answers how many instructions left the loop.
fn hoist_header_reads(
    func: &mut Function,
    lp: &NaturalLoop,
    pre: BlockId,
    v: ValueId,
    multi: &HashSet<ValueId>,
) -> u32 {
    if func.value_type(v) != Type::Str || multi.contains(&v) {
        return 0;
    }
    let body: Vec<usize> = lp.body.iter().map(|b| b.0 as usize).collect();
    let body_insts = || body.iter().flat_map(|&b| &func.blocks[b].insts);
    // the loop only reads through `v`: nothing in it is handed the
    // string, stores through it or releases it
    let reads_only = body_insts().all(|inst| {
        let mut names = false;
        visit_value_operands(&inst.kind, |o| names |= o == v);
        !names
            || matches!(
                &inst.kind,
                InstKind::Load(_, Operand::Value(p), _)
                | InstKind::LoadU8Dyn(Operand::Value(p), _)
                | InstKind::LoadU16Dyn(Operand::Value(p), _) if *p == v
            )
            || matches!(inst.kind, InstKind::ICmp(..) | InstKind::RcInc(_))
    });
    if !reads_only || body_insts().any(|i| i.result == Some(v)) {
        return 0;
    }
    let mut uses: HashMap<ValueId, u32> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |o| *uses.entry(o).or_insert(0) += 1);
        }
        if let Terminator::CondBr {
            cond: Operand::Value(c),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(c))) = &block.term
        {
            *uses.entry(*c).or_insert(0) += 1;
        }
    }
    let single = |x: ValueId| uses.get(&x) == Some(&1);

    // what each moved or folded instruction's result is from now on
    let mut same: HashMap<ValueId, ValueId> = HashMap::new();
    let (mut len, mut wide_len, mut latin1): (Option<ValueId>, Option<ValueId>, Option<ValueId>) =
        (None, None, None);
    let mut out: Vec<Inst> = Vec::new();
    let mut gone = 0u32;
    for &b in &body {
        let insts = std::mem::take(&mut func.blocks[b].insts);
        let mut keep: Vec<Inst> = Vec::with_capacity(insts.len());
        let mut i = 0;
        while i < insts.len() {
            let inst = &insts[i];
            let r = inst.result;
            let slot = match &inst.kind {
                InstKind::Load(Type::I32, Operand::Value(p), STR_LEN_OFF) if *p == v => {
                    Some((&mut len, 1))
                }
                InstKind::ZExtI32ToI64(Operand::Value(x))
                    if len.is_some() && same.get(x).or(Some(x)) == len.as_ref() =>
                {
                    Some((&mut wide_len, 1))
                }
                InstKind::Load(Type::I64, Operand::Value(p), 0)
                    if *p == v && is_latin1_test(&insts[i..], &single) =>
                {
                    Some((&mut latin1, 3))
                }
                _ => None,
            };
            match (slot, r) {
                (Some((first, n)), Some(_)) => {
                    let last = insts[i + n - 1].result.expect("a value");
                    match *first {
                        Some(kept) => {
                            same.insert(last, kept);
                        }
                        None => {
                            *first = Some(last);
                            out.extend(insts[i..i + n].iter().cloned());
                        }
                    }
                    gone += n as u32;
                    i += n;
                }
                _ => {
                    keep.push(inst.clone());
                    i += 1;
                }
            }
        }
        func.blocks[b].insts = keep;
    }
    if gone == 0 {
        return 0;
    }
    func.blocks[pre.0 as usize].insts.extend(out);
    if !same.is_empty() {
        let to = |x: ValueId| same.get(&x).copied().unwrap_or(x);
        for block in func.blocks.iter_mut() {
            for inst in block.insts.iter_mut() {
                rewrite_value_operands(&mut inst.kind, to);
            }
            if let Terminator::CondBr {
                cond: Operand::Value(c),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(c))) = &mut block.term
            {
                *c = to(*c);
            }
        }
    }
    gone
}

/// `insts` opens with `t = load i64 v+0; m = and t, IS_LATIN1;
/// c = icmp ne m, 0`, each the only reader of the one before.
fn is_latin1_test(insts: &[Inst], single: &dyn Fn(ValueId) -> bool) -> bool {
    let [load, and, cmp, ..] = insts else {
        return false;
    };
    let (Some(t), Some(m)) = (load.result, and.result) else {
        return false;
    };
    single(t)
        && single(m)
        && cmp.result.is_some()
        && matches!(&and.kind,
            InstKind::BinOp(BinOp::And, Operand::Value(a), Operand::ConstI64(STR_IS_LATIN1)) if *a == t)
        && matches!(&cmp.kind,
            InstKind::ICmp(IPred::Ne, Operand::Value(a), Operand::ConstI64(0)) if *a == m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, FuncId, THROW_ACTIVE_SYM, ValueInfo};

    fn v(i: u32) -> Operand {
        Operand::Value(ValueId(i))
    }
    fn mk(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }
    fn blk(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }
    fn cbr(c: u32, t: u32, e: u32) -> Terminator {
        Terminator::CondBr {
            cond: v(c),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        }
    }

    /// `while (i < s.length) { s.charCodeAt(i) … }` as lowering leaves
    /// it, %0 the string and %7 the counter:
    ///
    ///   b0: %1 = s == null, i = 0 → b1
    ///   b1 (header): %1 ? b2 : b3
    ///   b2 (raise): call, probe ? b9 : b10
    ///   b3: %3 = s == @U ? b2 : b4
    ///   b4: %4 = len, %5 = zext, %6 = i < %5 ? b5 : b8
    ///   b5: %10 = latin1 bit ? b6 : b7 ; b6 → b7
    ///   b7 (latch): `latch`, i = i + 1 → b1
    ///   b8: ret ; b9: ret ; b10: unreachable
    fn scanner(latch: Vec<Inst>) -> Function {
        let ty = |ty: Type| ValueInfo { ty, name: None };
        let mut values = vec![ty(Type::Str), ty(Type::Bool), ty(Type::Ptr), ty(Type::Bool)];
        values.extend([ty(Type::I32), ty(Type::I64), ty(Type::Bool), ty(Type::I64)]);
        values.extend([ty(Type::I64), ty(Type::I64), ty(Type::Bool), ty(Type::I64)]);
        values.extend([ty(Type::Ptr), ty(Type::I64), ty(Type::Bool), ty(Type::I64)]);
        let mut step = latch;
        step.push(mk(
            11,
            InstKind::BinOp(BinOp::Add, v(7), Operand::ConstI64(1)),
        ));
        step.push(mk(7, InstKind::Copy(Type::I64, v(11))));
        Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::I64,
            values,
            blocks: vec![
                blk(
                    0,
                    vec![
                        mk(1, InstKind::ICmp(IPred::Eq, v(0), Operand::ConstPtrNull)),
                        mk(7, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                blk(1, vec![], cbr(1, 2, 3)),
                blk(
                    2,
                    vec![
                        Inst {
                            result: None,
                            kind: InstKind::Call(FuncId(0), vec![v(0)]),
                            origin: None,
                        },
                        mk(12, InstKind::GlobalRef(THROW_ACTIVE_SYM.into())),
                        mk(13, InstKind::Load(Type::I64, v(12), 0)),
                        mk(14, InstKind::ICmp(IPred::Ne, v(13), Operand::ConstI64(0))),
                    ],
                    cbr(14, 9, 10),
                ),
                blk(
                    3,
                    vec![
                        mk(2, InstKind::GlobalRef("U".into())),
                        mk(3, InstKind::ICmp(IPred::Eq, v(0), v(2))),
                    ],
                    cbr(3, 2, 4),
                ),
                blk(
                    4,
                    vec![
                        mk(4, InstKind::Load(Type::I32, v(0), STR_LEN_OFF)),
                        mk(5, InstKind::ZExtI32ToI64(v(4))),
                        mk(6, InstKind::ICmp(IPred::Slt, v(7), v(5))),
                    ],
                    cbr(6, 5, 8),
                ),
                blk(
                    5,
                    vec![
                        mk(8, InstKind::Load(Type::I64, v(0), 0)),
                        mk(
                            9,
                            InstKind::BinOp(BinOp::And, v(8), Operand::ConstI64(STR_IS_LATIN1)),
                        ),
                        mk(10, InstKind::ICmp(IPred::Ne, v(9), Operand::ConstI64(0))),
                    ],
                    cbr(10, 6, 7),
                ),
                blk(6, vec![], Terminator::Br(BlockId(7))),
                blk(7, step, Terminator::Br(BlockId(1))),
                blk(8, vec![], Terminator::Ret(Some(v(7)))),
                blk(9, vec![], Terminator::Ret(Some(Operand::ConstI64(0)))),
                blk(10, vec![], Terminator::Unreachable),
            ],
            current_origin: None,
        }
    }

    fn run(f: &mut Function) -> LoopGuardStats {
        let mut stats = LoopGuardStats::default();
        hoist_fn(f, &mut stats);
        stats
    }

    #[test]
    fn a_scanner_tests_and_measures_its_string_once() {
        let mut f = scanner(vec![]);
        let stats = run(&mut f);
        assert_eq!((stats.guards_hoisted, stats.reads_hoisted), (2, 5));
        // the header chain falls straight through to the bound test
        assert!(matches!(f.blocks[1].term, Terminator::Br(BlockId(3))));
        assert!(matches!(f.blocks[3].term, Terminator::Br(BlockId(4))));
        assert!(f.blocks[3].insts.is_empty());
        assert_eq!(f.blocks[4].insts.len(), 1);
        assert!(f.blocks[5].insts.is_empty());
        // entry → null test → sentinel test → preheader → header
        let Terminator::Br(null_test) = f.blocks[0].term else {
            panic!("entry branches on");
        };
        let Terminator::CondBr {
            then_blk: BlockId(2),
            else_blk: sentinel_test,
            ..
        } = f.blocks[null_test.0 as usize].term
        else {
            panic!("the null test raises on true");
        };
        let sentinel = &f.blocks[sentinel_test.0 as usize];
        assert_eq!(sentinel.insts.len(), 2);
        let Terminator::CondBr {
            then_blk: BlockId(2),
            else_blk: pre,
            ..
        } = sentinel.term
        else {
            panic!("the sentinel test raises on true");
        };
        let pre = &f.blocks[pre.0 as usize];
        assert_eq!(pre.insts.len(), 5);
        assert!(matches!(pre.term, Terminator::Br(BlockId(1))));
    }

    #[test]
    fn a_loop_that_hands_its_string_to_a_call_keeps_its_reads() {
        let call = Inst {
            result: None,
            kind: InstKind::Call(FuncId(0), vec![v(0)]),
            origin: None,
        };
        let mut f = scanner(vec![call]);
        let stats = run(&mut f);
        assert_eq!((stats.guards_hoisted, stats.reads_hoisted), (2, 0));
        assert_eq!(f.blocks[4].insts.len(), 3);
    }

    #[test]
    fn a_test_of_a_value_the_loop_steps_stays() {
        // the header tests the counter, not the string
        let mut f = scanner(vec![]);
        f.blocks[0].insts.remove(0);
        f.blocks[1].insts = vec![mk(1, InstKind::ICmp(IPred::Eq, v(7), Operand::ConstI64(9)))];
        assert_eq!(run(&mut f).guards_hoisted, 0);
        assert!(matches!(f.blocks[1].term, Terminator::CondBr { .. }));
    }
}
