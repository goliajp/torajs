//! Lanes packed two to a vector register: which kept cells freeze
//! themselves, which slots stay scalar, how many pairs fit, and that a
//! jammed loop never holds more vectors than there are registers.

use super::escape::{Reads, escape};
use super::interp::run;
use super::*;
use crate::id_hash::HashSet;
use torajs_core::ssa::{FPred, IPred, visit_value_operands};

fn f(x: f64) -> Operand {
    Operand::ConstF64(x)
}

fn plan_for(func: &Function) -> Plan {
    plan::find_plan(func, &Default::default()).expect("the j loop is planned")
}

/// `n` more f64 values every lane computes ahead of its inner loop
/// (`ci · (i + 2)`), each added to the total after it; where
/// `in_step`, the step adds them to `zi` too.
fn extra_lane_values(mut func: Function, n: usize, in_step: bool) -> Function {
    let fresh = |func: &mut Function| {
        func.values.push(ValueInfo {
            ty: Type::F64,
            name: None,
        });
        func.values.len() as u32 - 1
    };
    let mut extra = Vec::new();
    for i in 0..n {
        let e = fresh(&mut func);
        let scale = InstKind::BinOp(BinOp::FMul, v(9), f(i as f64 + 2.0));
        func.blocks[2].insts.push(inst(e, scale));
        extra.push(e);
    }
    // b8: 25 = total·0.5, 26 = 25 + r, 27 = j + 1, total = 26, j = 27
    let mut acc = 26;
    let mut sums = Vec::new();
    for &e in &extra {
        let s = fresh(&mut func);
        sums.push(inst(s, InstKind::BinOp(BinOp::FAdd, v(acc), v(e))));
        acc = s;
    }
    let b8 = &mut func.blocks[8].insts;
    b8.splice(3..3, sums);
    b8[3 + n] = inst(4, InstKind::Copy(Type::F64, v(acc)));
    if in_step {
        // b5: 22 = 21 + ci, then zi = 22: add each one ahead of it
        let mut acc = 22;
        let mut adds = Vec::new();
        for &e in &extra {
            let s = fresh(&mut func);
            adds.push(inst(s, InstKind::BinOp(BinOp::FAdd, v(acc), v(e))));
            acc = s;
        }
        let b5 = &mut func.blocks[5].insts;
        let at = b5
            .iter()
            .position(|i| i.result == Some(ValueId(11)))
            .expect("zi");
        b5[at] = inst(11, InstKind::Copy(Type::F64, v(acc)));
        b5.splice(at..at, adds);
    }
    func
}

/// The most vectors live at once anywhere in `func`, just before or
/// just after an instruction; a cell counts from a write to its last
/// read before the next write.
fn max_live_vectors(func: &Function) -> usize {
    let is_vec = |x: ValueId| matches!(func.value_type(x), Type::Vec(_));
    let n = func.blocks.len();
    let mut live_in: Vec<HashSet<ValueId>> = vec![HashSet::default(); n];
    let successors = |b: &Block| -> Vec<usize> {
        match b.term {
            Terminator::Br(t) => vec![t.0 as usize],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => vec![then_blk.0 as usize, else_blk.0 as usize],
            _ => vec![],
        }
    };
    let backward = |b: &Block, live_in: &[HashSet<ValueId>], mut peak: Option<&mut usize>| {
        let mut live: HashSet<ValueId> = HashSet::default();
        for s in successors(b) {
            live.extend(live_in[s].iter().copied());
        }
        for inst in b.insts.iter().rev() {
            // after the instruction: what lives on, and a result nothing
            // reads still takes a register
            let unread = inst.result.is_some_and(|r| is_vec(r) && !live.contains(&r));
            let after = live.len() + usize::from(unread);
            if let Some(r) = inst.result {
                live.remove(&r);
            }
            visit_value_operands(&inst.kind, |x| {
                if is_vec(x) {
                    live.insert(x);
                }
            });
            // an operand read for the last time may hand its register
            // to the result
            if let Some(peak) = peak.as_deref_mut() {
                *peak = (*peak).max(after).max(live.len());
            }
        }
        live
    };
    let mut changed = true;
    while changed {
        changed = false;
        for b in func.blocks.iter().rev() {
            let live = backward(b, &live_in, None);
            if live != live_in[b.id.0 as usize] {
                live_in[b.id.0 as usize] = live;
                changed = true;
            }
        }
    }
    let mut peak = 0;
    for b in &func.blocks {
        backward(b, &live_in, Some(&mut peak));
    }
    peak
}

fn args_grid() -> Vec<[i64; 4]> {
    let fl = |x: f64| x.to_bits() as i64;
    let mut out = Vec::new();
    for bound in [0, 1, 3, 5, 7, 12] {
        for max in [0, 1, 2, 3, 5, 20] {
            for cr in [-2.0, -0.75, 0.26, 2.0, f64::NAN] {
                for lim in [4.0, 0.0, 1.5] {
                    out.push([bound, max, fl(cr), fl(lim)]);
                }
            }
        }
    }
    out
}

fn same_results(func: &Function, what: &str) {
    let (g, n) = jam(func);
    assert_eq!(n, 1, "{what}");
    for args in args_grid() {
        assert_eq!(
            run(func, &args, 1 << 22),
            run(&g, &args, 1 << 24),
            "{what} args {args:?}"
        );
    }
}

/// A bump ahead of the last exit test, or a sum read before it, keeps
/// a snapshot: a lane leaving there has stepped its cell.
#[test]
fn a_cell_bumped_ahead_of_an_exit_does_not_freeze() {
    let frozen = |reads: Reads| {
        let func = escape(reads);
        let plan = plan_for(&func);
        assert_eq!(plan.exit_cells, Some(vec![ValueId(12)]), "{reads:?}");
        plan.packing.expect("packed").frozen
    };
    assert_eq!(frozen(Reads::Count), vec![(ValueId(12), ValueId(23))]);
    assert_eq!(frozen(Reads::CountAtHead), vec![]);
    assert_eq!(frozen(Reads::SumFirst), vec![]);
    for reads in [Reads::CountAtHead, Reads::SumFirst] {
        same_results(&escape(reads), &format!("{reads:?}"));
    }
}

/// Values only the code after the inner loop reads stay scalars: seven
/// of them beside the point leave one pair of eight vectors.
#[test]
fn a_value_only_post_reads_stays_scalar() {
    let func = extra_lane_values(escape(Reads::Point), 7, false);
    let pk = plan_for(&func).packing.expect("packed");
    assert_eq!((pk.per_pair, pk.splats, pk.temps, pk.lanes), (8, 4, 3, 2));
    assert_eq!(pk.packed.len(), 4);
    same_results(&func, "post reads seven");
    let (g, _) = jam(&func);
    assert!(max_live_vectors(&g) <= 21, "{}", max_live_vectors(&g));
}

/// Seven more values the step reads make fifteen vectors a pair, more
/// than the fourteen registers beside the splats and temporaries: the
/// group keeps its scalar form.
#[test]
fn a_pair_that_does_not_fit_keeps_the_scalar_form() {
    let func = extra_lane_values(escape(Reads::Point), 7, true);
    let plan = plan_for(&func);
    assert!(plan.masked);
    assert!(plan.packing.is_none());
    same_results(&func, "step reads seven");
    let (g, _) = jam(&func);
    assert_eq!(max_live_vectors(&g), 0);
}

#[test]
fn a_packed_group_holds_no_more_vectors_than_registers() {
    for reads in [
        Reads::Count,
        Reads::HeadZr,
        Reads::Point,
        Reads::BumpFirst,
        Reads::CountAtHead,
        Reads::SumFirst,
    ] {
        let (g, _) = jam(&escape(reads));
        assert!(max_live_vectors(&g) > 0, "{reads:?} packs");
        assert!(
            max_live_vectors(&g) <= 21,
            "{reads:?} {}",
            max_live_vectors(&g)
        );
    }
}

/// An exit testing a bool the lane computed ahead of its inner loop
/// has no lane mask to fold: the group keeps its scalar form.
#[test]
fn an_exit_on_a_bool_slot_keeps_the_scalar_form() {
    let mut func = escape(Reads::Count);
    let flag = func.values.len() as u32;
    func.values.push(ValueInfo {
        ty: Type::Bool,
        name: None,
    });
    let test = InstKind::FCmp(FPred::Ogt, v(9), f(0.0));
    func.blocks[2].insts.push(inst(flag, test));
    // b4 leaves on `ci > 0` instead of the escape test
    func.blocks[4].term = cbr(flag, 7, 5);
    let plan = plan_for(&func);
    assert!(plan.masked);
    assert!(plan.packing.is_none());
    same_results(&func, "bool slot exit");
    // and on a compare of the counter's bound alone, a value from
    // outside the loop, likewise
    let mut func = escape(Reads::Count);
    let flag = func.values.len() as u32;
    func.values.push(ValueInfo {
        ty: Type::Bool,
        name: None,
    });
    let test = InstKind::ICmp(IPred::Sgt, v(1), c(3));
    func.blocks[0].insts.push(inst(flag, test));
    func.blocks[4].term = cbr(flag, 7, 5);
    let plan = plan_for(&func);
    assert!(plan.packing.is_none());
    same_results(&func, "invariant bool exit");
}
