use super::*;

#[test]
fn cost_arithmetic_saturates() {
    assert_eq!(Cost::new(0).as_u32(), 0);
    assert_eq!(Cost::new(100).add(Cost::new(50)).as_u32(), 150);
    // Saturation: adding HUGE to anything stays HUGE.
    assert_eq!(Cost::HUGE.add(Cost::new(1)), Cost::HUGE);
    // mul saturates.
    assert_eq!(Cost::new(u32::MAX / 8).mul(2), Cost::HUGE);
}

#[test]
fn loop_depth_scales_exponentially() {
    let body = Cost::new(5);
    assert_eq!(body.scale_for_depth(0), body);
    assert_eq!(body.scale_for_depth(1), Cost::new(50));
    assert_eq!(body.scale_for_depth(2), Cost::new(500));
    // Deep nest saturates at HUGE rather than wrapping.
    assert_eq!(body.scale_for_depth(20), Cost::HUGE);
}

#[test]
fn opcode_costs_match_pipeline_units() {
    use torajs_core::ssa::{IPred, Operand, Type};
    let dummy = Operand::ConstI64(0);

    // Integer ALU and shifts are 1 cycle.
    assert_eq!(
        cost_of_kind(&InstKind::BinOp(BinOp::Add, dummy, dummy)),
        ALU_COST
    );
    assert_eq!(
        cost_of_kind(&InstKind::BinOp(BinOp::Shl, dummy, dummy)),
        ALU_COST
    );

    // Mul is 3, sdiv is 10, fadd is 2, fmul is 4, fdiv is 15.
    assert_eq!(
        cost_of_kind(&InstKind::BinOp(BinOp::Mul, dummy, dummy)),
        MUL_COST
    );
    assert_eq!(
        cost_of_kind(&InstKind::BinOp(BinOp::SDiv, dummy, dummy)),
        DIV_COST
    );
    assert_eq!(
        cost_of_kind(&InstKind::BinOp(BinOp::FAdd, dummy, dummy)),
        FALU_COST
    );
    assert_eq!(
        cost_of_kind(&InstKind::BinOp(BinOp::FMul, dummy, dummy)),
        FMUL_COST
    );
    assert_eq!(
        cost_of_kind(&InstKind::BinOp(BinOp::FDiv, dummy, dummy)),
        FDIV_COST
    );

    // Memory + compare + call families.
    assert_eq!(
        cost_of_kind(&InstKind::Load(Type::I64, dummy, 0)),
        LOAD_COST
    );
    assert_eq!(cost_of_kind(&InstKind::Store(dummy, dummy, 0)), STORE_COST);
    assert_eq!(
        cost_of_kind(&InstKind::ICmp(IPred::Eq, dummy, dummy)),
        ALU_COST
    );
    assert_eq!(
        cost_of_kind(&InstKind::Call(torajs_core::ssa::FuncId(0), vec![])),
        CALL_COST
    );

    // Alloca folds into prologue.
    assert_eq!(cost_of_kind(&InstKind::Alloca(Type::I64)), Cost::ZERO);
    assert_eq!(cost_of_kind(&InstKind::AllocaBytes(32)), Cost::ZERO);

    // Casts cross the integer↔fp pipeline.
    assert_eq!(cost_of_kind(&InstKind::SiToFp(dummy)), CAST_COST);
    assert_eq!(cost_of_kind(&InstKind::FpToSi(dummy)), CAST_COST);
    assert_eq!(
        cost_of_kind(&InstKind::FpToInt32(dummy)),
        CAST_COST.add(ALU_COST)
    );
}

#[test]
fn terminator_costs_classify_by_unit() {
    use torajs_core::ssa::{BlockId, Operand};
    assert_eq!(cost_of_terminator(&Terminator::Br(BlockId(0))), BRANCH_COST);
    assert_eq!(
        cost_of_terminator(&Terminator::CondBr {
            cond: Operand::ConstBool(true),
            then_blk: BlockId(0),
            else_blk: BlockId(1),
        }),
        BRANCH_COST
    );
    assert_eq!(cost_of_terminator(&Terminator::Ret(None)), ALU_COST);
    assert_eq!(cost_of_terminator(&Terminator::Unreachable), Cost::ZERO);
}

#[test]
fn cost_ordering_supports_extraction() {
    // Extraction picks the lowest-cost representation of an eclass;
    // verify the ordering relation matches the numeric expectation.
    assert!(Cost::ZERO < ALU_COST);
    assert!(ALU_COST < MUL_COST);
    assert!(MUL_COST < DIV_COST);
    assert!(LOAD_COST > STORE_COST);
    assert!(FDIV_COST > FMUL_COST);
    assert!(FMUL_COST > FALU_COST);
}
