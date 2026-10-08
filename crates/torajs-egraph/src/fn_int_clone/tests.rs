use super::*;
use torajs_core::ssa::{BinOp, Block, BlockId, FPred, Inst, Terminator, ValueInfo};

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

fn func(name: &str, params: u32, tys: &[Type], ret: Type, blocks: Vec<Block>) -> Function {
    Function {
        name: name.into(),
        params: (0..params).map(ValueId).collect(),
        ret,
        blocks,
        values: tys
            .iter()
            .map(|ty| ValueInfo {
                ty: *ty,
                name: None,
            })
            .collect(),
        current_origin: None,
    }
}

fn decl(name: &str) -> Function {
    func(name, 0, &[], Type::F64, vec![])
}

/// `fib` (fn 0) with its second argument built by `second`, `main`
/// (fn 1) calling it on what `src` (fn 2) answers, `mark` (fn 3).
///
/// ```text
/// bb0: %1 = fcmp olt %0, 2 ; cond_br %1, bb1, bb2
/// bb1: ret %0
/// bb2: %2 = fsub %0, 1 ; %3 = call fib(%2) ; %4 = <second>
///      %5 = call fib(%4) ; %6 = fadd %3, %5 ; ret %6
/// ```
fn module(second: InstKind, extra: Option<Inst>) -> Module {
    let mut body = vec![
        inst(
            2,
            InstKind::BinOp(BinOp::FSub, v(0), Operand::ConstF64(1.0)),
        ),
        inst(3, InstKind::Call(FuncId(0), vec![v(2)])),
        inst(4, second),
        inst(5, InstKind::Call(FuncId(0), vec![v(4)])),
        inst(6, InstKind::BinOp(BinOp::FAdd, v(3), v(5))),
    ];
    body.extend(extra);
    let fib = func(
        "fib",
        1,
        &[
            Type::F64,
            Type::Bool,
            Type::F64,
            Type::F64,
            Type::F64,
            Type::F64,
            Type::F64,
        ],
        Type::F64,
        vec![
            Block {
                id: BlockId(0),
                insts: vec![inst(
                    1,
                    InstKind::FCmp(FPred::Olt, v(0), Operand::ConstF64(2.0)),
                )],
                term: Terminator::CondBr {
                    cond: v(1),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![],
                term: Terminator::Ret(Some(v(0))),
            },
            Block {
                id: BlockId(2),
                insts: body,
                term: Terminator::Ret(Some(v(6))),
            },
        ],
    );
    let main = func(
        "main",
        0,
        &[Type::F64, Type::F64],
        Type::F64,
        vec![Block {
            id: BlockId(0),
            insts: vec![
                inst(0, InstKind::Call(FuncId(2), vec![])),
                inst(1, InstKind::Call(FuncId(0), vec![v(0)])),
            ],
            term: Terminator::Ret(Some(v(1))),
        }],
    );
    Module {
        funcs: vec![fib, main, decl("src"), decl(MARK_ARGUMENTS)],
        ..Default::default()
    }
}

fn fsub(c: f64) -> InstKind {
    InstKind::BinOp(BinOp::FSub, v(0), Operand::ConstF64(c))
}

fn calls(f: &Function, callee: u32) -> usize {
    f.blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter(|i| matches!(&i.kind, InstKind::Call(c, _) if c.0 == callee))
        .count()
}

#[test]
fn clones_a_recursion_closed_over_integers() {
    let mut m = module(fsub(2.0), None);
    let stats = clone_functions(&mut m);
    assert_eq!(stats.functions_cloned, 1);
    assert_eq!(stats.clones, vec![(FuncId(0), FuncId(4))]);
    // both arguments can leave the window only below it
    assert_eq!((stats.internal_sites, stats.internal_checks), (2, 2));
    assert_eq!(stats.external_sites, 1);
    let clone = &m.funcs[4];
    assert_eq!(clone.values[clone.params[0].0 as usize].ty, Type::I64);
    let fcmps = clone
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter(|i| matches!(i.kind, InstKind::FCmp(..)))
        .count();
    assert_eq!(fcmps, 0);
    assert_eq!((calls(clone, 4), calls(clone, 0)), (2, 2));
    // main checks once and keeps the original on the failing arm
    assert_eq!((calls(&m.funcs[1], 4), calls(&m.funcs[1], 0)), (1, 1));
}

#[test]
fn an_argument_that_is_not_affine_keeps_the_function_whole() {
    let mut m = module(
        InstKind::BinOp(BinOp::FMul, v(0), Operand::ConstF64(0.5)),
        None,
    );
    assert_eq!(clone_functions(&mut m).functions_cloned, 0);
    assert_eq!(m.funcs.len(), 4);
}

#[test]
fn a_function_reading_arguments_is_not_cloned() {
    let mark = Inst {
        result: None,
        kind: InstKind::Call(FuncId(3), vec![]),
        origin: None,
    };
    let mut m = module(fsub(2.0), Some(mark));
    assert_eq!(clone_functions(&mut m).functions_cloned, 0);
}

#[test]
fn a_fractional_constant_argument_keeps_the_function_whole() {
    let mut m = module(fsub(0.5), None);
    assert_eq!(clone_functions(&mut m).functions_cloned, 0);
}

#[test]
fn without_a_direct_caller_there_is_no_clone() {
    let mut m = module(fsub(2.0), None);
    m.funcs[1].blocks[0].insts.pop();
    m.funcs[1].blocks[0].term = Terminator::Ret(Some(v(0)));
    assert_eq!(clone_functions(&mut m).functions_cloned, 0);
}

#[test]
fn the_entry_assumptions_go_once_read() {
    let mut m = module(fsub(2.0), None);
    let stats = clone_functions(&mut m);
    let late = crate::strip_assumes::strip_assumes(&mut m, &stats.clones);
    assert_eq!(late.assumes_stripped, 2);
    let clone = &m.funcs[4];
    assert!(
        clone
            .blocks
            .iter()
            .all(|b| b.term != Terminator::Unreachable)
    );
    // the two window checks were not folded here: both still dispatch
    assert_eq!((late.dispatch_checks_left, late.slow_calls_left), (2, 2));
}

#[test]
fn a_trap_the_lowering_sealed_stays_in_the_clone() {
    let mut m = module(fsub(2.0), None);
    let fib = &mut m.funcs[0];
    fib.blocks[1].term = Terminator::CondBr {
        cond: v(1),
        then_blk: BlockId(4),
        else_blk: BlockId(3),
    };
    for (id, term) in [
        (3, Terminator::Unreachable),
        (4, Terminator::Ret(Some(v(0)))),
    ] {
        fib.blocks.push(Block {
            id: BlockId(id),
            insts: vec![],
            term,
        });
    }
    let stats = clone_functions(&mut m);
    let late = crate::strip_assumes::strip_assumes(&mut m, &stats.clones);
    assert_eq!(late.assumes_stripped, 2);
    let clone = &m.funcs[4];
    let traps: Vec<BlockId> = clone
        .blocks
        .iter()
        .filter(|b| b.insts.is_empty() && b.term == Terminator::Unreachable)
        .map(|b| b.id)
        .collect();
    assert!(clone.blocks.iter().any(|b| matches!(
        b.term,
        Terminator::CondBr { then_blk, else_blk, .. }
            if traps.contains(&then_blk) || traps.contains(&else_blk)
    )));
}

mod loops;
