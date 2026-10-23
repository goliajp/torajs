use super::*;
use crate::interval;
use torajs_core::ssa::{Block, FPred, Inst, Terminator, Type, ValueInfo};

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

fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
    Block {
        id: BlockId(id),
        insts,
        term,
    }
}

/// `for (i = 0; i < n; i++) total += i`, `n` the f64 a call answered:
///
/// ```text
/// bb0: %0 = call f()            ; the bound
///      %1 = copy i64 0          ; i
///      %2 = copy i64 0          ; total
///      br bb1
/// bb1: %3 = sitofp %1
///      %4 = fcmp <pred> %3, %0
///      cond_br %4, bb2, bb3
/// bb2: %5 = add %2, %1
///      %6 = add %1, 1
///      %2 = copy i64 %5
///      %1 = copy i64 %6
///      br bb1
/// bb3: ret %2
/// ```
pub(super) fn counting_loop(pred: FPred, body_extra: Vec<Inst>) -> Module {
    let tys = [
        Type::F64,
        Type::I64,
        Type::I64,
        Type::F64,
        Type::Bool,
        Type::I64,
        Type::I64,
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
    let mut body = vec![
        inst(5, InstKind::BinOp(BinOp::Add, v(2), v(1))),
        inst(6, InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(1))),
    ];
    body.extend(body_extra);
    body.push(inst(2, InstKind::Copy(Type::I64, v(5))));
    body.push(inst(1, InstKind::Copy(Type::I64, v(6))));
    let f = Function {
        name: "f".into(),
        params: vec![],
        ret: Type::I64,
        blocks: vec![
            block(
                0,
                vec![
                    inst(0, InstKind::Call(torajs_core::ssa::FuncId(0), vec![])),
                    inst(1, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
                    inst(2, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
                ],
                Terminator::Br(BlockId(1)),
            ),
            block(
                1,
                vec![
                    inst(3, InstKind::SiToFp(v(1))),
                    inst(4, InstKind::FCmp(pred, v(3), v(0))),
                ],
                Terminator::CondBr {
                    cond: v(4),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            ),
            block(2, body, Terminator::Br(BlockId(1))),
            block(3, vec![], Terminator::Ret(Some(v(2)))),
        ],
        values,
        current_origin: None,
    };
    let callee = Function {
        name: "bound".into(),
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

/// The integer compare of the fast header, as (counter, bound).
fn fast_compare(f: &Function, pred: IPred) -> (ValueId, ValueId) {
    f.blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .find_map(|i| match &i.kind {
            InstKind::ICmp(p, Operand::Value(c), Operand::Value(b)) if *p == pred => Some((*c, *b)),
            _ => None,
        })
        .expect("an integer compare between two values")
}

#[test]
fn the_copy_compares_integers_and_the_loop_stays_as_it_was() {
    let mut m = counting_loop(FPred::Olt, vec![]);
    let stats = version_loops(&mut m);
    assert_eq!(stats.loops_versioned, 1);
    assert_eq!(stats.bounds_converted, 1);
    let f = &m.funcs[1];
    // the loop itself still compares f64s, and loops back to itself
    assert!(matches!(
        &f.blocks[1].insts[1].kind,
        InstKind::FCmp(FPred::Olt, _, Operand::Value(ValueId(0)))
    ));
    assert!(matches!(f.blocks[2].term, Terminator::Br(BlockId(1))));
    // the copy's header compares its own counter with an i64
    let (counter, bound) = fast_compare(f, IPred::Slt);
    assert_ne!(counter, ValueId(1), "the copy counts in its own cell");
    assert_eq!(f.values[bound.0 as usize].ty, Type::I64);
    assert!(matches!(f.blocks[5].term, Terminator::Br(BlockId(4))));
    // the entry goes through the chain, not into either header
    let Terminator::Br(chain) = f.blocks[0].term else {
        panic!("entry terminator");
    };
    assert!(chain.0 >= 6, "chain blocks come after the copy");
}

#[test]
fn the_copy_counter_starts_from_what_the_preheader_stored() {
    let mut m = counting_loop(FPred::Olt, vec![]);
    version_loops(&mut m);
    let f = &m.funcs[1];
    let (counter, _) = fast_compare(f, IPred::Slt);
    let seeded = f.blocks.iter().flat_map(|b| b.insts.iter()).any(|i| {
        i.result == Some(counter) && i.kind == InstKind::Copy(Type::I64, Operand::ConstI64(0))
    });
    assert!(seeded, "seeded from the stored constant, not from the cell");
}

#[test]
fn a_cell_only_the_loop_stores_is_not_seeded() {
    // a second cell, stored in the body alone
    let extra = vec![
        inst(8, InstKind::Copy(Type::I64, v(1))),
        inst(8, InstKind::Copy(Type::I64, v(5))),
    ];
    let mut m = counting_loop(FPred::Olt, extra);
    version_loops(&mut m);
    let f = &m.funcs[1];
    let reads_the_slow_cell = f.blocks[6..]
        .iter()
        .flat_map(|b| b.insts.iter())
        .any(|i| i.kind == InstKind::Copy(Type::I64, v(8)));
    assert!(!reads_the_slow_cell);
}

#[test]
fn the_copy_counter_is_bounded_by_the_window() {
    let mut m = counting_loop(FPred::Olt, vec![]);
    // a step of 64 is past the small steps the interval analysis
    // bounds by their trip horizon, so only the window bounds `i`
    for inst in &mut m.funcs[1].blocks[2].insts {
        if inst.result == Some(ValueId(6)) {
            inst.kind = InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(64));
        }
    }
    version_loops(&mut m);
    let f = &m.funcs[1];
    let (counter, _) = fast_compare(f, IPred::Slt);
    let facts = interval::analyze_function(f);
    let i = facts.get(&counter).expect("the counter has a fact");
    // below the window on the test, plus the one step taken after it
    assert_eq!((i.lo, i.hi), (0, EXACT_MAX as i128 + 63));
    // the loop as it was bounds nothing
    let slow = facts.get(&ValueId(1)).expect("the cell has a fact");
    assert!(slow.hi > EXACT_MAX as i128);
}

#[test]
fn a_loop_doing_bitwise_work_gets_the_int32_window() {
    let extra = vec![inst(
        7,
        InstKind::BinOp(BinOp::And, v(1), Operand::ConstI64(255)),
    )];
    let mut m = counting_loop(FPred::Olt, extra);
    version_loops(&mut m);
    let f = &m.funcs[1];
    let (counter, _) = fast_compare(f, IPred::Slt);
    let facts = interval::analyze_function(f);
    let i = facts.get(&counter).expect("the counter has a fact");
    // `i < bound`, bound at most 2^31
    assert_eq!((i.lo, i.hi), (0, 1 << 31));
}

#[test]
fn the_code_after_the_loop_reads_a_cell_both_copies_store_into() {
    let mut m = counting_loop(FPred::Olt, vec![]);
    version_loops(&mut m);
    let f = &m.funcs[1];
    let Terminator::Ret(Some(Operand::Value(out))) = f.blocks[3].term else {
        panic!("exit terminator");
    };
    assert_ne!(out, ValueId(2), "the reader left the loop's cell");
    let stored_from: Vec<Operand> = f
        .blocks
        .iter()
        .filter(|b| matches!(b.term, Terminator::Br(BlockId(3))))
        .flat_map(|b| b.insts.iter())
        .filter(|i| i.result == Some(out))
        .map(|i| match &i.kind {
            InstKind::Copy(_, from) => *from,
            other => panic!("not a copy: {other:?}"),
        })
        .collect();
    assert_eq!(stored_from.len(), 2, "one store on each copy's exit");
    assert!(stored_from.contains(&v(2)), "the loop's own total");
    // the counter is not read again: no out cell for it
    let outs = f
        .blocks
        .iter()
        .filter(|b| matches!(b.term, Terminator::Br(BlockId(3))))
        .map(|b| b.insts.len())
        .collect::<Vec<_>>();
    assert_eq!(outs, vec![1, 1]);
}

#[test]
fn a_cell_read_by_code_that_also_runs_before_the_loop_keeps_it_unversioned() {
    let mut m = counting_loop(FPred::Olt, vec![]);
    // the exit block loops back to the entry, which reads the total
    let f = &mut m.funcs[1];
    f.blocks[3].term = Terminator::Br(BlockId(0));
    f.blocks[0].insts.insert(
        0,
        inst(8, InstKind::BinOp(BinOp::Add, v(2), Operand::ConstI64(1))),
    );
    let stats = version_loops(&mut m);
    assert_eq!(stats.loops_versioned, 0);
    assert_eq!(stats.skipped_escaping, 1);
}

#[test]
fn an_inclusive_compare_rounds_the_bound_down() {
    let mut m = counting_loop(FPred::Ole, vec![]);
    version_loops(&mut m);
    let f = &m.funcs[1];
    fast_compare(f, IPred::Sle);
    let floors = f
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter(|i| matches!(i.kind, InstKind::FCmp(FPred::Ogt, ..)))
        .count();
    assert_eq!(floors, 1, "one floor adjustment, in the chain");
}

/// The value the copy reads in a source's place: the `sitofp` of the
/// integer the chain passed.
fn image_of(f: &Function, source: ValueId, op: &Operand) -> bool {
    let Operand::Value(x) = op else {
        return false;
    };
    if *x == source {
        return false;
    }
    let def = |v: ValueId| {
        f.blocks
            .iter()
            .flat_map(|b| b.insts.iter())
            .find(|i| i.result == Some(v))
            .map(|i| i.kind.clone())
    };
    match def(*x) {
        Some(InstKind::SiToFp(Operand::Value(pi))) => match def(pi) {
            Some(InstKind::Copy(Type::I64, Operand::Value(t))) => {
                matches!(def(t), Some(InstKind::FpToSi(_)))
            }
            _ => false,
        },
        _ => false,
    }
}

#[test]
fn a_bound_the_loop_also_adds_is_taken_as_it_is() {
    let extra = vec![inst(
        7,
        InstKind::BinOp(BinOp::FAdd, v(0), Operand::ConstF64(1.0)),
    )];
    let mut m = counting_loop(FPred::Olt, extra);
    // value 7 is an f64 here
    m.funcs[1].values[7].ty = Type::F64;
    let stats = version_loops(&mut m);
    assert_eq!(stats.loops_versioned, 1);
    assert_eq!(stats.bounds_converted, 1);
    assert_eq!(stats.values_converted, 1);
    let f = &m.funcs[1];
    // the copy compares with the integer itself, no rounding in the chain
    let (_, bound) = fast_compare(f, IPred::Slt);
    assert_eq!(f.values[bound.0 as usize].ty, Type::I64);
    let rounds = f
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter(|i| matches!(i.kind, InstKind::FCmp(FPred::Olt | FPred::Ogt, ..)))
        .count();
    assert_eq!(rounds, 1, "only the loop's own compare");
    // the chain compares the bit patterns of the value and its round trip
    let bit_compares = f
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter(|i| matches!(i.kind, InstKind::BitCastF64ToI64(_)))
        .count();
    assert_eq!(bit_compares, 2);
    // the copy adds the image, the loop as it was still adds the source
    let adds: Vec<&Operand> = f
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter_map(|i| match &i.kind {
            InstKind::BinOp(BinOp::FAdd, a, _) => Some(a),
            _ => None,
        })
        .collect();
    assert_eq!(adds.len(), 2);
    assert!(adds.iter().any(|a| **a == v(0)));
    assert!(adds.iter().any(|a| image_of(f, ValueId(0), a)));
}

#[test]
fn a_value_the_loop_divides_by_gets_the_divisor_window() {
    // `total = total + (i % m)` with `m` a second opaque f64
    let extra = vec![inst(7, InstKind::BinOp(BinOp::FRem, v(3), v(9)))];
    let mut m = counting_loop(FPred::Olt, extra);
    m.funcs[1].values[7].ty = Type::F64;
    m.funcs[1].values.push(ValueInfo {
        ty: Type::F64,
        name: None,
    });
    m.funcs[1].blocks[0].insts.insert(
        1,
        inst(9, InstKind::Call(torajs_core::ssa::FuncId(0), vec![])),
    );
    let stats = version_loops(&mut m);
    assert_eq!(stats.loops_versioned, 1);
    assert_eq!(stats.values_converted, 1);
    let f = &m.funcs[1];
    let rems: Vec<&Operand> = f
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter_map(|i| match &i.kind {
            InstKind::BinOp(BinOp::FRem, _, d) => Some(d),
            _ => None,
        })
        .collect();
    assert_eq!(rems.len(), 2);
    assert!(rems.iter().any(|d| image_of(f, ValueId(9), d)));
    // the window's floor is 1: a divisor of 0 takes the loop as it was
    let floor_one = f
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .any(|i| matches!(i.kind, InstKind::ICmp(IPred::Slt, _, Operand::ConstI64(1))));
    assert!(floor_one);
    // and the image has that fact
    let image = f
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .find_map(|i| match (&i.kind, i.result) {
            (InstKind::SiToFp(Operand::Value(pi)), Some(r)) if image_of(f, ValueId(9), &v(r.0)) => {
                Some(*pi)
            }
            _ => None,
        })
        .expect("the image");
    let facts = interval::analyze_function(f);
    let fact = facts.get(&image).expect("the integer has a fact");
    assert_eq!((fact.lo, fact.hi), (1, EXACT_VALUE_MAX as i128));
}

#[test]
fn a_value_read_after_the_loop_keeps_it_unversioned() {
    let mut m = counting_loop(FPred::Olt, vec![]);
    // a cold exit out of the body returns the body's single-def sum;
    // bb2 dominates it, so the read is well formed
    let f = &mut m.funcs[1];
    f.blocks[2].term = Terminator::CondBr {
        cond: v(4),
        then_blk: BlockId(5),
        else_blk: BlockId(4),
    };
    f.blocks.push(block(4, vec![], Terminator::Ret(Some(v(5)))));
    f.blocks.push(block(5, vec![], Terminator::Br(BlockId(1))));
    let stats = version_loops(&mut m);
    assert_eq!(stats.loops_versioned, 0);
    assert_eq!(stats.skipped_escaping, 1);
}

#[test]
fn a_cell_started_from_a_source_runs_on_its_image() {
    // `seed = (seed * 48271) % 2147483647` each trip, `seed` started
    // from a second opaque f64 in the entry block
    let extra = vec![
        inst(
            7,
            InstKind::BinOp(BinOp::FMul, v(10), Operand::ConstF64(48271.0)),
        ),
        inst(
            11,
            InstKind::BinOp(BinOp::FRem, v(7), Operand::ConstF64(2147483647.0)),
        ),
        inst(10, InstKind::Copy(Type::F64, v(11))),
    ];
    let mut m = counting_loop(FPred::Olt, extra);
    let f = &mut m.funcs[1];
    f.values[7].ty = Type::F64;
    for _ in 9..12 {
        f.values.push(ValueInfo {
            ty: Type::F64,
            name: None,
        });
    }
    f.blocks[0].insts.insert(
        1,
        inst(9, InstKind::Call(torajs_core::ssa::FuncId(0), vec![])),
    );
    f.blocks[0]
        .insts
        .insert(2, inst(10, InstKind::Copy(Type::F64, v(9))));
    let stats = version_loops(&mut m);
    assert_eq!(stats.loops_versioned, 1);
    assert_eq!(stats.values_converted, 1);
    let f = &m.funcs[1];
    // the copy's cell starts from the image of the source
    let seeded = f.blocks.iter().flat_map(|b| b.insts.iter()).any(
        |i| matches!(&i.kind, InstKind::Copy(Type::F64, from) if image_of(f, ValueId(9), from)),
    );
    assert!(seeded);
    // the window keeps the product inside 2^53 and the dividend at or above 0
    let top = EXACT_VALUE_MAX / 48271;
    let checks: Vec<i64> = f
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter_map(|i| match i.kind {
            InstKind::ICmp(IPred::Sgt | IPred::Slt, _, Operand::ConstI64(c)) => Some(c),
            _ => None,
        })
        .collect();
    assert!(checks.contains(&top), "{checks:?}");
    assert!(checks.contains(&0), "{checks:?}");
}
