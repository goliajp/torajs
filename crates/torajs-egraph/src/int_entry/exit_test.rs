//! The integer exit test, for a loop whose bound informs nothing but the
//! test itself.
//!
//! Copying a loop pays when the copy's integer bound tells the passes
//! after it something about the values the body computes from the
//! counter. When it tells them nothing ([`demand`](super::demand)),
//! all the copy buys is the exit test done on integers. That test needs
//! no copy: a test that is
//! true only where the f64 one is true may run first, with the f64 test
//! behind it for the rest.
//!
//! For an integer `i`, `i < ceil(n)` implies `i < n` for every `n`
//! whose rounding is exact, and `sitofp` only ever rounds an `i` past
//! the window towards it, so the implication holds for every `i` once
//! the rounded bound lies inside the window. The header now reads
//! `i < nb ? body : (sitofp i < n ? body : exit)`: `nb` is the rounded
//! bound when it lies inside the window, and otherwise (NaN, an
//! infinity, anything past 2^52) a value no `i` is below, so the f64
//! test decides every round. `<=` compares with `floor(n) + 1`, `>`
//! and `>=` mirror it with `floor(n)` and `ceil(n) - 1` and the
//! largest i64.
//!
//! The f64 test runs once, on the way out, and the loop is not
//! duplicated. So the test is only for one that leaves every loop
//! holding it: one that exits once per round of an outer loop would pay
//! the f64 test every round, where the copy pays nothing.

use std::collections::hash_map::Entry;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::loop_analysis::LoopAnalysis;
use torajs_core::ssa::{
    BinOp, BlockId, FPred, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use super::guard::Round;
use super::{BoundUse, EXACT_MAX};
use crate::cfg_edit::{emit, new_block};
use crate::float_demote::cfg::{retarget_edge, successors, term_operand_value};
use crate::float_demote::push_value;
use crate::rc_peephole::visit_value_operands;

/// One f64 exit test to put an integer test in front of.
struct Site {
    block: BlockId,
    source: ValueId,
    round: Round,
    pred: IPred,
    /// the compared image is read by the compare alone
    own_image: bool,
}

pub(super) struct ExitTest {
    header: BlockId,
    region: HashSet<BlockId>,
    sites: Vec<Site>,
}

/// One f64 test of a versioned loop, by the value its compare defines:
/// an id that holds while later passes renumber the blocks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TestAt {
    pub(super) test: ValueId,
    pub(super) source: ValueId,
    pub(super) round: Round,
    pub(super) pred: IPred,
}

/// The plan for a loop whose copy turned out to buy nothing but its
/// integer tests: each `(block, test)` the block branching on it.
pub(super) fn of_tests(
    header: BlockId,
    region: HashSet<BlockId>,
    tests: &[(BlockId, TestAt)],
) -> ExitTest {
    let sites = tests
        .iter()
        .map(|(block, t)| Site {
            block: *block,
            source: t.source,
            round: t.round,
            pred: t.pred,
            own_image: true,
        })
        .collect();
    ExitTest {
        header,
        region,
        sites,
    }
}

/// How many times each value is read, by an instruction or a branch.
pub(super) fn reads_of(func: &Function) -> HashMap<ValueId, usize> {
    let mut reads: HashMap<ValueId, usize> = HashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| *reads.entry(v).or_default() += 1);
        }
        if let Some(v) = term_operand_value(&block.term) {
            *reads.entry(v).or_default() += 1;
        }
    }
    reads
}

/// The exit test plan of a loop whose bound informs nothing but the
/// exit test (see `demand`): every compared image made in its compare's
/// block, every compare read only by its block's branch and leaving
/// every loop holding it, and no bound taken as it is. A test inside
/// another loop exits once per outer round, each exit paying the f64
/// test a copy would not; such a loop is copied. `reads` counts the
/// reads of the whole function.
pub(super) fn plan(
    func: &Function,
    loops: &LoopAnalysis,
    header: BlockId,
    region: &HashSet<BlockId>,
    uses: &[BoundUse],
    reads: &HashMap<ValueId, usize>,
) -> Option<ExitTest> {
    let served = |u: &BoundUse| u.round != Round::Exact && leaves_every_loop(func, loops, u.block);
    if uses.is_empty() || !uses.iter().all(served) {
        return None;
    }
    let mut sites = Vec::new();
    for u in uses {
        let block = &func.blocks[u.block.0 as usize];
        let InstKind::FCmp(_, Operand::Value(l), Operand::Value(r)) = &block.insts[u.inst].kind
        else {
            return None;
        };
        let image = if *r == u.source { *l } else { *r };
        let test = block.insts[u.inst].result?;
        let image_here = block.insts[..u.inst]
            .iter()
            .any(|i| i.result == Some(image) && matches!(i.kind, InstKind::SiToFp(_)));
        let branches_on_test = matches!(
            block.term,
            Terminator::CondBr { cond: Operand::Value(c), .. } if c == test
        );
        if !image_here || !branches_on_test || reads.get(&test) != Some(&1) {
            return None;
        }
        sites.push(Site {
            block: u.block,
            source: u.source,
            round: u.round,
            pred: u.pred,
            own_image: reads.get(&image) == Some(&1),
        });
    }
    Some(ExitTest {
        header,
        region: region.clone(),
        sites,
    })
}

/// One of the block's successors lies outside every loop holding it.
pub(super) fn leaves_every_loop(func: &Function, loops: &LoopAnalysis, b: BlockId) -> bool {
    let holding: Vec<_> = loops.loops().iter().filter(|l| l.contains(&b)).collect();
    successors(&func.blocks[b.0 as usize].term).any(|s| holding.iter().all(|l| !l.contains(&s)))
}

impl ExitTest {
    pub(super) fn header(&self) -> BlockId {
        self.header
    }
}

/// The integer test in front of each f64 one, and the rounded bounds it
/// reads, computed once on the way into the loop. Answers the block
/// that computes them, which every way into the loop now enters.
pub(super) fn apply(func: &mut Function, t: &ExitTest) -> BlockId {
    let outside = func.blocks.len();
    let entry = new_block(func);
    let mut bound_of: HashMap<(ValueId, IPred), Operand> = HashMap::default();
    for s in &t.sites {
        if let Entry::Vacant(slot) = bound_of.entry((s.source, s.pred)) {
            slot.insert(rounded_bound(func, entry, s));
        }
    }
    func.blocks[entry.0 as usize].term = Terminator::Br(t.header);
    for bi in 0..outside {
        if !t.region.contains(&BlockId(bi as u32)) {
            retarget_edge(&mut func.blocks[bi].term, t.header, entry);
        }
    }
    for s in &t.sites {
        join_test(func, s, bound_of[&(s.source, s.pred)]);
    }
    entry
}

/// The integer bound of one test, branch-free: `n` truncated and
/// moved to its ceiling or floor (and by one for `<=` / `>=`) when it
/// lies inside the window, where its rounding does too and the
/// truncation is exact; otherwise (NaN fails both window compares) a
/// bound no counter passes.
fn rounded_bound(func: &mut Function, at: BlockId, s: &Site) -> Operand {
    let n = Operand::Value(s.source);
    let mut e = |ty, kind| emit(func, at, ty, kind);
    let t = e(Type::I64, InstKind::FpToSi(n));
    let back = e(Type::F64, InstKind::SiToFp(t));
    let (pred, op) = match s.round {
        Round::Ceil => (FPred::Olt, BinOp::Add),
        _ => (FPred::Ogt, BinOp::Sub),
    };
    let off = e(Type::Bool, InstKind::FCmp(pred, back, n));
    let off = e(Type::I64, InstKind::ZExtBoolToI64(off));
    let r = e(Type::I64, InstKind::BinOp(op, t, off));
    let w = EXACT_MAX as f64;
    let lo = e(
        Type::Bool,
        InstKind::FCmp(FPred::Oge, n, Operand::ConstF64(-w)),
    );
    let hi = e(
        Type::Bool,
        InstKind::FCmp(FPred::Ole, n, Operand::ConstF64(w)),
    );
    let ok = e(Type::Bool, InstKind::BinOp(BinOp::And, lo, hi));
    let (step, never) = match s.pred {
        IPred::Slt => (0, i64::MIN),
        IPred::Sle => (1, i64::MIN),
        IPred::Sgt => (0, i64::MAX),
        _ => (-1, i64::MAX),
    };
    let r = match step {
        0 => r,
        k => e(
            Type::I64,
            InstKind::BinOp(BinOp::Add, r, Operand::ConstI64(k)),
        ),
    };
    e(
        Type::I64,
        InstKind::Select(Type::I64, ok, r, Operand::ConstI64(never)),
    )
}

/// `c = fcmp (sitofp i), n; br c, then, else` becomes `f = icmp i, nb;
/// o = or f, c; br o, then, else`. The loop keeps one test through the
/// passes that read its shape; `exit_split`, after them, runs the f64
/// test only where the integer one fails. An image the loop reads
/// elsewhere stays where it is, and the f64 test gets one of its own.
fn join_test(func: &mut Function, s: &Site, bound: Operand) {
    let fresh = (!s.own_image).then(|| push_value(&mut func.values, Type::F64));
    let block = &mut func.blocks[s.block.0 as usize];
    let Terminator::CondBr {
        cond: Operand::Value(test),
        ..
    } = block.term
    else {
        unreachable!("the plan checked the branch");
    };
    // the f64 test and its image go last, for the split to find
    let take = |insts: &mut Vec<Inst>, v: ValueId| {
        let at = insts.iter().position(|i| i.result == Some(v));
        insts.remove(at.expect("the plan found it in the block"))
    };
    let mut test_inst = take(&mut block.insts, test);
    let InstKind::FCmp(_, Operand::Value(l), Operand::Value(r)) = test_inst.kind else {
        unreachable!("the plan checked the compare");
    };
    let image = if r == s.source { l } else { r };
    let image_inst = match fresh {
        None => take(&mut block.insts, image),
        Some(own) => own_image(&block.insts, image, own, &mut test_inst),
    };
    let InstKind::SiToFp(counter) = image_inst.kind else {
        unreachable!("the plan checked the image");
    };
    let pred = match s.pred {
        IPred::Slt | IPred::Sle => IPred::Slt,
        _ => IPred::Sgt,
    };
    let fast = emit(
        func,
        s.block,
        Type::Bool,
        InstKind::ICmp(pred, counter, bound),
    );
    let insts = &mut func.blocks[s.block.0 as usize].insts;
    insts.push(image_inst);
    insts.push(test_inst);
    let either = emit(
        func,
        s.block,
        Type::Bool,
        InstKind::BinOp(BinOp::Or, fast, Operand::Value(test)),
    );
    if let Terminator::CondBr { cond, .. } = &mut func.blocks[s.block.0 as usize].term {
        *cond = either;
    }
}

/// A second `sitofp` of the counter, `own`, for the f64 test to read
/// in place of the `image` the loop reads elsewhere.
fn own_image(insts: &[Inst], image: ValueId, own: ValueId, test: &mut Inst) -> Inst {
    let def = insts.iter().find(|i| i.result == Some(image));
    let kind = def.expect("the plan found it in the block").kind.clone();
    if let InstKind::FCmp(_, l, r) = &mut test.kind {
        for o in [l, r] {
            if matches!(o, Operand::Value(v) if *v == image) {
                *o = Operand::Value(own);
            }
        }
    }
    Inst {
        result: Some(own),
        kind,
        origin: None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::version_loops;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, FPred, FuncId, Function, IPred, Inst, InstKind, Module, Operand,
        Terminator, Type, ValueId, ValueInfo,
    };

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn inst(result: Option<u32>, kind: InstKind) -> Inst {
        Inst {
            result: result.map(ValueId),
            kind,
            origin: None,
        }
    }

    /// `for (i = 0; i <pred> n; i++) g(i)` with `n` a call's answer;
    /// `extra` joins the body.
    fn counting(pred: FPred, extra: Vec<Inst>) -> Module {
        let tys = [
            Type::F64,
            Type::I64,
            Type::F64,
            Type::Bool,
            Type::I64,
            Type::I64,
            Type::F64,
            Type::F64,
            Type::F64,
        ];
        let values = tys
            .iter()
            .map(|ty| ValueInfo {
                ty: *ty,
                name: None,
            })
            .collect();
        let mut body = vec![inst(None, InstKind::Call(FuncId(0), vec![v(1)]))];
        body.extend(extra);
        body.push(inst(
            Some(4),
            InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(1)),
        ));
        body.push(inst(Some(1), InstKind::Copy(Type::I64, v(4))));
        let header = vec![
            inst(Some(2), InstKind::SiToFp(v(1))),
            inst(Some(3), InstKind::FCmp(pred, v(2), v(0))),
        ];
        let entry = vec![
            inst(Some(0), InstKind::Call(FuncId(0), vec![])),
            inst(Some(1), InstKind::Copy(Type::I64, Operand::ConstI64(0))),
        ];
        let block = |id: u32, insts: Vec<Inst>, term: Terminator| Block {
            id: BlockId(id),
            insts,
            term,
        };
        let blocks = vec![
            block(0, entry, Terminator::Br(BlockId(1))),
            block(
                1,
                header,
                Terminator::CondBr {
                    cond: v(3),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            ),
            block(2, body, Terminator::Br(BlockId(1))),
            block(3, vec![], Terminator::Ret(Some(Operand::ConstI64(0)))),
        ];
        let f = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            blocks,
            values,
            current_origin: None,
        };
        let callee = Function {
            name: "g".into(),
            params: vec![],
            ret: Type::F64,
            blocks: vec![],
            values: vec![],
            current_origin: None,
        };
        Module {
            funcs: vec![callee, f],
            ..Module::default()
        }
    }

    #[test]
    fn a_counter_that_only_counts_gets_an_integer_test_and_no_copy() {
        let mut m = counting(FPred::Olt, vec![]);
        let stats = version_loops(&mut m);
        assert_eq!((stats.loops_versioned, stats.exit_tests), (0, 1));
        let f = &m.funcs[1];
        let Terminator::CondBr {
            cond: Operand::Value(either),
            then_blk: BlockId(2),
            else_blk: BlockId(3),
        } = f.blocks[1].term
        else {
            panic!("the header keeps its targets");
        };
        let def = |v: ValueId| f.blocks[1].insts.iter().find(|i| i.result == Some(v));
        let Some(InstKind::BinOp(BinOp::Or, Operand::Value(fast), Operand::Value(ValueId(3)))) =
            def(either).map(|i| &i.kind)
        else {
            panic!("the integer test or the f64 one");
        };
        assert!(matches!(
            def(*fast).map(|i| &i.kind),
            Some(InstKind::ICmp(IPred::Slt, Operand::Value(ValueId(1)), _))
        ));
        // the entry goes through the block that rounds the bound
        assert!(matches!(f.blocks[0].term, Terminator::Br(b) if b.0 > 3));
    }

    #[test]
    fn a_counter_the_body_computes_with_still_gets_the_copy() {
        let extra = vec![inst(Some(5), InstKind::BinOp(BinOp::Mul, v(1), v(1)))];
        let mut m = counting(FPred::Olt, extra);
        let stats = version_loops(&mut m);
        assert_eq!((stats.loops_versioned, stats.exit_tests), (1, 0));
    }

    fn versioned_and_tested(extra: Vec<Inst>) -> (u32, u32) {
        let stats = version_loops(&mut counting(FPred::Olt, extra));
        (stats.loops_versioned, stats.exit_tests)
    }

    #[test]
    fn the_counters_image_is_followed_into_f64() {
        let image = || inst(Some(6), InstKind::SiToFp(v(1)));
        let summed = vec![
            image(),
            inst(Some(7), InstKind::BinOp(BinOp::FAdd, v(6), v(0))),
        ];
        assert_eq!(versioned_and_tested(summed), (1, 0));
        let handed_over = vec![image(), inst(None, InstKind::Call(FuncId(0), vec![v(6)]))];
        assert_eq!(versioned_and_tested(handed_over), (0, 1));
    }

    #[test]
    fn a_sum_with_a_value_no_fact_bounds_is_not_one_a_bound_informs() {
        let summed = vec![
            inst(Some(8), InstKind::Call(FuncId(0), vec![])),
            inst(Some(6), InstKind::SiToFp(v(1))),
            inst(Some(7), InstKind::BinOp(BinOp::FAdd, v(8), v(6))),
        ];
        assert_eq!(versioned_and_tested(summed), (0, 1));
    }

    #[test]
    fn a_counter_shifted_or_divided_by_a_constant_only_counts() {
        let shifted = vec![inst(
            Some(5),
            InstKind::BinOp(BinOp::Shl, v(1), Operand::ConstI64(32)),
        )];
        assert_eq!(versioned_and_tested(shifted), (0, 1));
        let rem = vec![inst(
            Some(5),
            InstKind::BinOp(BinOp::SRem, v(1), Operand::ConstI64(4)),
        )];
        assert_eq!(versioned_and_tested(rem), (0, 1));
        let mut m = counting(
            FPred::Ogt,
            vec![inst(
                Some(5),
                InstKind::BinOp(BinOp::SRem, v(1), Operand::ConstI64(4)),
            )],
        );
        let stats = version_loops(&mut m);
        assert_eq!(
            stats.loops_versioned, 1,
            "a bound from below sets the remainder's sign"
        );
    }

    #[test]
    fn a_compared_image_read_elsewhere_stays_and_the_f64_test_gets_its_own() {
        let shared = vec![inst(None, InstKind::Call(FuncId(0), vec![v(2)]))];
        let mut m = counting(FPred::Olt, shared);
        let stats = version_loops(&mut m);
        assert_eq!((stats.loops_versioned, stats.exit_tests), (0, 1));
        let header = &m.funcs[1].blocks[1].insts;
        assert!(
            matches!(header[0].kind, InstKind::SiToFp(_)) && header[0].result == Some(ValueId(2))
        );
        let n = header.len();
        let Some(own) = header[n - 3].result else {
            panic!("the f64 test's image is defined");
        };
        assert!(own != ValueId(2) && matches!(header[n - 3].kind, InstKind::SiToFp(_)));
        assert!(matches!(header[n - 2].kind, InstKind::FCmp(_, Operand::Value(x), _) if x == own));
    }

    #[test]
    fn at_most_compares_below_the_floor_plus_one() {
        let mut m = counting(FPred::Ole, vec![]);
        let stats = version_loops(&mut m);
        assert_eq!(stats.exit_tests, 1);
        let insts: Vec<&Inst> = m.funcs[1].blocks.iter().flat_map(|b| &b.insts).collect();
        let steps_up = insts.iter().any(|i| {
            i.result != Some(ValueId(4))
                && matches!(i.kind, InstKind::BinOp(BinOp::Add, _, Operand::ConstI64(1)))
        });
        assert!(steps_up, "floor(n) + 1 is the integer bound");
        let never = |i: &&Inst| {
            matches!(
                i.kind,
                InstKind::Select(_, _, _, Operand::ConstI64(i64::MIN))
            )
        };
        assert!(
            insts.iter().any(never),
            "a bound outside the window is one no counter is below"
        );
    }
}
