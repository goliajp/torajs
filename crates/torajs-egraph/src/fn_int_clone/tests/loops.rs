//! Calls from inside loops: where their conversion test goes.

use super::*;

/// `main` calling `fib` in a loop, on `src()` read before the loop or,
/// with `inside`, on every trip.
///
/// ```text
/// bb0: [%0 = call src()] ; br bb1
/// bb1: [%0 = call src()] ; %1 = call fib(%0) ; %2 = fcmp olt %1, 10
///      cond_br %2, bb1, bb2
/// bb2: ret %1
/// ```
fn looping_main(inside: bool) -> Function {
    let read = inst(0, InstKind::Call(FuncId(2), vec![]));
    let mut pre = vec![];
    let mut body = vec![];
    if inside { &mut body } else { &mut pre }.push(read);
    body.push(inst(1, InstKind::Call(FuncId(0), vec![v(0)])));
    body.push(inst(
        2,
        InstKind::FCmp(FPred::Olt, v(1), Operand::ConstF64(10.0)),
    ));
    let blocks = vec![
        Block {
            id: BlockId(0),
            insts: pre,
            term: Terminator::Br(BlockId(1)),
        },
        Block {
            id: BlockId(1),
            insts: body,
            term: Terminator::CondBr {
                cond: v(2),
                then_blk: BlockId(1),
                else_blk: BlockId(2),
            },
        },
        Block {
            id: BlockId(2),
            insts: vec![],
            term: Terminator::Ret(Some(v(1))),
        },
    ];
    func(
        "main",
        0,
        &[Type::F64, Type::F64, Type::Bool],
        Type::F64,
        blocks,
    )
}

#[test]
fn a_call_in_a_loop_converts_its_argument_in_front_of_the_loop() {
    let mut m = module(fsub(2.0), None);
    m.funcs[1] = looping_main(false);
    let stats = clone_functions(&mut m);
    assert_eq!((stats.functions_cloned, stats.external_sites), (1, 1));
    let main = &m.funcs[1];
    let converts = |b: &Block| {
        b.insts
            .iter()
            .any(|i| matches!(i.kind, InstKind::FpToSi(Operand::Value(ValueId(0)))))
    };
    assert!(converts(&main.blocks[0]));
    assert!(main.blocks[1..].iter().all(|b| !converts(b)));
    // the loop only branches on the test
    assert!(matches!(
        main.blocks[1].term,
        Terminator::CondBr { cond: Operand::Value(c), .. }
            if main.blocks[0].insts.iter().any(|i| i.result == Some(c))
    ));
    assert_eq!((calls(main, 4), calls(main, 0)), (1, 1));
}

#[test]
fn a_call_in_a_loop_on_a_changing_argument_keeps_the_original() {
    let mut m = module(fsub(2.0), None);
    m.funcs[1] = looping_main(true);
    assert_eq!(clone_functions(&mut m).functions_cloned, 0);
    assert_eq!(calls(&m.funcs[1], 0), 1);
}

/// `main` with the block in front of its loop numbered after the
/// loop, holding a call routed at its own site ahead of the source
/// the loop's hoisted test converts.
///
/// ```text
/// bb0: br bb2
/// bb1: %1 = call fib(%0) ; %2 = fcmp olt %1, 10 ; cond_br %2, bb1, bb3
/// bb2: %3 = call src() ; %4 = call fib(%3) ; %0 = call src() ; br bb1
/// bb3: ret %1
/// ```
#[test]
fn a_call_split_off_the_tests_block_leaves_the_test_its_source() {
    let mut m = module(fsub(2.0), None);
    let mut main = looping_main(false);
    main.blocks[0].term = Terminator::Br(BlockId(2));
    main.blocks[1].term = Terminator::CondBr {
        cond: v(2),
        then_blk: BlockId(1),
        else_blk: BlockId(3),
    };
    main.blocks[2].insts = vec![
        inst(3, InstKind::Call(FuncId(2), vec![])),
        inst(4, InstKind::Call(FuncId(0), vec![v(3)])),
        main.blocks[0].insts.remove(0),
    ];
    main.blocks[2].term = Terminator::Br(BlockId(1));
    main.blocks.push(Block {
        id: BlockId(3),
        insts: vec![],
        term: Terminator::Ret(Some(v(1))),
    });
    main.values
        .extend([Type::F64, Type::F64].map(|ty| ValueInfo { ty, name: None }));
    m.funcs[1] = main;
    let stats = clone_functions(&mut m);
    assert_eq!((stats.functions_cloned, stats.external_sites), (1, 2));
    let main = &m.funcs[1];
    let find = |want: &dyn Fn(&Inst) -> bool| {
        main.blocks.iter().find_map(|b| {
            let i = b.insts.iter().position(want)?;
            Some((b.id, i))
        })
    };
    let def = find(&|i| i.result == Some(ValueId(0))).expect("the source is defined");
    let test = find(&|i| matches!(i.kind, InstKind::FpToSi(Operand::Value(ValueId(0)))))
        .expect("the loop's call is converted in front of the loop");
    // the conversion reads the source after it is written
    assert_eq!(test.0, def.0);
    assert!(test.1 > def.1);
}
