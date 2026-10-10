use super::*;

#[test]
fn self_recursive_call_is_rejected_at_depth_zero() {
    let main = caller("main", vec![void_inst(InstKind::Call(FuncId(0), vec![]))]);
    let mut m = module_of(vec![main]);
    let budget = InlineBudget {
        max_recursion_depth: 0,
        ..InlineBudget::default()
    };
    let stats = inline_module_with_budget(&mut m, budget);
    assert_eq!(stats.candidates, 1);
    assert_eq!(stats.skipped_recursion, 1);
    assert_eq!(stats.would_inline, 0);
}

/// fib-shaped self-recursive callee: 2 self-call sites, 2
/// value-bearing rets. At the Phase 2.1 default
/// (`max_recursion_depth: 1`) both sites inline ONE level — each
/// splice clones the ORIGINAL 2-site body (taken from the
/// pre-splice snapshot), so the result holds exactly 4 recursive
/// call sites. A compounding clone (snapshot bug) would yield 6.
fn fib_like() -> Function {
    Function {
        name: "fib".into(),
        params: vec![ValueId(0)],
        ret: Type::I64,
        blocks: vec![
            Block {
                id: BlockId(0),
                insts: vec![val_inst(
                    ValueId(1),
                    InstKind::ICmp(
                        torajs_core::ssa::IPred::Slt,
                        Operand::Value(ValueId(0)),
                        Operand::ConstI64(2),
                    ),
                )],
                term: Terminator::CondBr {
                    cond: Operand::Value(ValueId(1)),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![],
                term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
            },
            Block {
                id: BlockId(2),
                insts: vec![
                    val_inst(
                        ValueId(2),
                        InstKind::Call(FuncId(0), vec![Operand::Value(ValueId(0))]),
                    ),
                    val_inst(
                        ValueId(3),
                        InstKind::Call(FuncId(0), vec![Operand::Value(ValueId(0))]),
                    ),
                    val_inst(
                        ValueId(4),
                        InstKind::BinOp(
                            BinOp::Add,
                            Operand::Value(ValueId(2)),
                            Operand::Value(ValueId(3)),
                        ),
                    ),
                ],
                term: Terminator::Ret(Some(Operand::Value(ValueId(4)))),
            },
        ],
        values: vec![
            ValueInfo {
                ty: Type::I64,
                name: Some("n".into()),
            },
            ValueInfo {
                ty: Type::Bool,
                name: None,
            },
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
        ],
        current_origin: None,
    }
}

#[test]
fn self_recursive_sites_inline_one_level_from_snapshot() {
    let mut m = module_of(vec![fib_like()]);
    let budget = InlineBudget {
        max_recursion_depth: 1,
        ..InlineBudget::default()
    };
    let stats = inline_module_with_budget(&mut m, budget);
    assert_eq!(stats.candidates, 2);
    assert_eq!(stats.would_inline, 2);
    assert_eq!(stats.inlined, 2, "both self-sites splice at depth 1");
    let recursive_sites: usize = m.funcs[0]
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter(|i| matches!(i.kind, InstKind::Call(FuncId(0), _)))
        .count();
    assert_eq!(
        recursive_sites, 4,
        "each of the 2 sites clones the ORIGINAL 2-site body (snapshot); \
         6 would mean the second splice cloned the already-grown body"
    );
}
