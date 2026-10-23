//! The integer exit test, for a loop whose bound informs nothing but the
//! test itself.
//!
//! Copying a loop pays when the copy's integer bound tells the passes
//! after it something about the values the body computes from the
//! counter. When the counter only counts — it is compared with the
//! bound, stepped by a constant, masked by one, handed to a call or
//! stored — the bound tells them nothing, and all the copy buys is the
//! exit test done on integers. That test needs no copy: a test that is
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
//! duplicated. So the test is only for a loop no other loop holds: one
//! that exits once per round of an outer loop would pay the f64 test
//! every round, where the copy pays nothing.

use std::collections::hash_map::Entry;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, BlockId, FPred, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use super::guard::Round;
use super::{BoundUse, EXACT_MAX};
use crate::cfg_edit::{emit, new_block};
use crate::float_demote::cfg::{retarget_edge, term_operand_value};
use crate::rc_peephole::visit_value_operands;

/// One f64 exit test to put an integer test in front of.
struct Site {
    block: BlockId,
    source: ValueId,
    round: Round,
    pred: IPred,
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
        })
        .collect();
    ExitTest {
        header,
        region,
        sites,
    }
}

/// The exit test plan of a loop, when its bound informs nothing but the
/// exit test: every compared image read only by its compare, which only
/// its block's branch reads, and the counter and every value the loop
/// reads (none of them starting a cell of the loop) only counted.
pub(super) fn plan(
    func: &Function,
    header: BlockId,
    region: &HashSet<BlockId>,
    uses: &[BoundUse],
    values: &[ValueId],
    seeds_cells: bool,
) -> Option<ExitTest> {
    if seeds_cells || uses.is_empty() {
        return None;
    }
    let matched: HashSet<(BlockId, usize)> = uses.iter().map(|u| (u.block, u.inst)).collect();
    let mut reads: HashMap<ValueId, usize> = HashMap::default();
    for block in &func.blocks {
        for (i, inst) in block.insts.iter().enumerate() {
            if !matched.contains(&(block.id, i)) {
                visit_value_operands(&inst.kind, |v| *reads.entry(v).or_default() += 1);
            }
        }
        if let Some(v) = term_operand_value(&block.term) {
            *reads.entry(v).or_default() += 1;
        }
    }
    let mut sites = Vec::new();
    let mut counters = Vec::new();
    let mut images: HashSet<ValueId> = HashSet::default();
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
        if !image_here
            || !branches_on_test
            || reads.contains_key(&image)
            || reads.get(&test) != Some(&1)
        {
            return None;
        }
        images.insert(image);
        counters.extend(match u.counter {
            Operand::Value(c) => Some(c),
            _ => None,
        });
        sites.push(Site {
            block: u.block,
            source: u.source,
            round: u.round,
            pred: u.pred,
        });
    }
    if sites.iter().any(|s| s.round == Round::Exact)
        || counts_beyond(func, region, &[&counters, values].concat(), &images)
    {
        return None;
    }
    Some(ExitTest {
        header,
        region: region.clone(),
        sites,
    })
}

/// Whether some instruction of the loop does more with the counter than
/// count: everything derived from it by a copy or a constant step or
/// scale is followed; a constant non-negative mask, a call argument, a stored
/// value and the compared image end the trail; anything else is a use
/// the bound could inform.
fn counts_beyond(
    func: &Function,
    region: &HashSet<BlockId>,
    counters: &[ValueId],
    images: &HashSet<ValueId>,
) -> bool {
    let mut derived: HashSet<ValueId> = counters.iter().copied().collect();
    let constant = |o: &Operand| !matches!(o, Operand::Value(_));
    loop {
        let before = derived.len();
        for b in region {
            let block = &func.blocks[b.0 as usize];
            for inst in &block.insts {
                let mut reads = false;
                visit_value_operands(&inst.kind, |v| reads |= derived.contains(&v));
                if !reads || inst.result.is_some_and(|r| images.contains(&r)) {
                    continue;
                }
                match &inst.kind {
                    InstKind::Copy(..) => derived.extend(inst.result),
                    InstKind::BinOp(
                        BinOp::Add
                        | BinOp::Sub
                        | BinOp::Mul
                        | BinOp::Shl
                        | BinOp::AShr
                        | BinOp::LShr,
                        a,
                        c,
                    ) if constant(a) || constant(c) => derived.extend(inst.result),
                    InstKind::BinOp(BinOp::And, _, Operand::ConstI64(k))
                    | InstKind::BinOp(BinOp::And, Operand::ConstI64(k), _)
                        if *k >= 0 => {}
                    InstKind::Call(..) | InstKind::CallIndirect(..) => {}
                    InstKind::Store(Operand::Value(v), _, _) if derived.contains(v) => {}
                    _ => return true,
                }
            }
            if term_operand_value(&block.term).is_some_and(|v| derived.contains(&v)) {
                return true;
            }
        }
        if derived.len() == before {
            return false;
        }
    }
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
/// test only where the integer one fails.
fn join_test(func: &mut Function, s: &Site, bound: Operand) {
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
    let test_inst = take(&mut block.insts, test);
    let InstKind::FCmp(_, Operand::Value(l), Operand::Value(r)) = test_inst.kind else {
        unreachable!("the plan checked the compare");
    };
    let image_inst = take(&mut block.insts, if r == s.source { l } else { r });
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
