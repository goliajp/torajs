//! A param's interval ends at its last read on any path, like any
//! other value's: the cases where that read is not the textual last
//! one (a backedge, a block laid out ahead of the code that runs
//! before it) must still cover it, and a param read only by the first
//! call must keep its argument register.

use super::*;
use crate::linear_scan::allocate_linear_scan;
use crate::reg::{Gpr, Reg};
use torajs_core::ssa::{Block, BlockId, FuncId, Inst, Type, ValueInfo};

fn info(ty: Type) -> ValueInfo {
    ValueInfo { ty, name: None }
}

fn inst(result: Option<u32>, kind: InstKind) -> Inst {
    Inst {
        result: result.map(ValueId),
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

fn func(params: u32, values: Vec<ValueInfo>, blocks: Vec<Block>) -> Function {
    Function {
        name: "params".into(),
        params: (0..params).map(ValueId).collect(),
        ret: Type::Void,
        values,
        blocks,
        current_origin: None,
    }
}

fn call(result: u32, args: Vec<Operand>) -> Inst {
    inst(Some(result), InstKind::Call(FuncId(9), args))
}

fn moves_from(f: &Function, arg: Gpr) -> bool {
    let alloc = allocate_linear_scan(f);
    alloc
        .param_entry_moves
        .iter()
        .any(|&(from, _)| from == Reg::Gpr(arg))
}

/// `fn thunk(p) { f(p) }`: the call is the last read, so the param
/// does not cross it and stays in x0 with no entry move.
#[test]
fn a_param_read_only_by_the_first_call_keeps_its_register() {
    let f = func(
        1,
        vec![info(Type::Ptr), info(Type::I64)],
        vec![block(
            0,
            vec![call(1, vec![Operand::Value(ValueId(0))])],
            Terminator::Ret(None),
        )],
    );
    assert_eq!(compute_intervals(&f)[&0], Interval { start: 0, end: 0 });
    let alloc = allocate_linear_scan(&f);
    assert_eq!(alloc.of(ValueId(0)), Reg::Gpr(Gpr::X0));
    assert!(alloc.param_entry_moves.is_empty());
    assert_eq!(alloc.used_callee_gpr_mask, 0);
}

/// The param is read in the loop header, the body calls: the textual
/// last read (slot 1) precedes the call (slot 3), but the backedge
/// brings control back to the read, so the param is live across the
/// call and must leave x0.
///
///   B0: br B1                       slot 0
///   B1: %2 = load %0; condbr %2     slots 1, 2
///   B2: %1 = call f(); br B1        slots 3, 4
///   B3: ret                         slot 5
#[test]
fn a_param_read_in_a_loop_header_spans_the_backedge() {
    let f = func(
        1,
        vec![info(Type::Ptr), info(Type::I64), info(Type::I64)],
        vec![
            block(0, Vec::new(), Terminator::Br(BlockId(1))),
            block(
                1,
                vec![inst(
                    Some(2),
                    InstKind::Load(Type::I64, Operand::Value(ValueId(0)), 0),
                )],
                Terminator::CondBr {
                    cond: Operand::Value(ValueId(2)),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            ),
            block(2, vec![call(1, Vec::new())], Terminator::Br(BlockId(1))),
            block(3, Vec::new(), Terminator::Ret(None)),
        ],
    );
    assert!(compute_intervals(&f)[&0].end >= 4);
    assert!(moves_from(&f, Gpr::X0));
}

/// The block that reads the param is laid out ahead of the block that
/// calls and then branches to it: the read's slot (1) is below the
/// call's (3), yet it runs after the call.
///
///   B0: br B2                       slot 0
///   B1: store %0 -> [%1 + 8]; ret   slots 1, 2
///   B2: %1 = call f(); br B1        slots 3, 4
#[test]
fn a_param_read_in_a_block_laid_out_before_the_call_crosses_it() {
    let f = func(
        1,
        vec![info(Type::I64), info(Type::Ptr)],
        vec![
            block(0, Vec::new(), Terminator::Br(BlockId(2))),
            block(
                1,
                vec![inst(
                    None,
                    InstKind::Store(Operand::Value(ValueId(0)), Operand::Value(ValueId(1)), 8),
                )],
                Terminator::Ret(None),
            ),
            block(2, vec![call(1, Vec::new())], Terminator::Br(BlockId(1))),
        ],
    );
    assert!(compute_intervals(&f)[&0].end >= 3);
    assert!(moves_from(&f, Gpr::X0));
}

/// Two params, the first read by the leading call, the second after
/// it: only the second moves to a callee-saved register.
#[test]
fn only_the_param_read_after_a_call_is_relocated() {
    let f = func(
        2,
        vec![info(Type::Ptr), info(Type::I64), info(Type::Ptr)],
        vec![block(
            0,
            vec![
                call(2, vec![Operand::Value(ValueId(0))]),
                inst(
                    None,
                    InstKind::Store(Operand::Value(ValueId(1)), Operand::Value(ValueId(2)), 8),
                ),
            ],
            Terminator::Ret(None),
        )],
    );
    let alloc = allocate_linear_scan(&f);
    assert_eq!(alloc.of(ValueId(0)), Reg::Gpr(Gpr::X0));
    let moved: Vec<Reg> = alloc
        .param_entry_moves
        .iter()
        .map(|&(from, _)| from)
        .collect();
    assert_eq!(moved, vec![Reg::Gpr(Gpr::X1)]);
}
