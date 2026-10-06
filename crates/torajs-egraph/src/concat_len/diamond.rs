//! The diamond one measured concat becomes.

use crate::cfg_edit::{emit, new_block, renumber};
use crate::float_demote::split_block_at;
use crate::str_append::kernels::{Kernels, Kind};

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use super::{STR_LEN_OFF, STR_MAX_LEN, SUBSTR_LEN_OFF};

/// head:    null tests on the Str operands → slow | measure
/// measure: lengths, sum, bound test → slow | fast
/// fast:    `n = sum` → join
/// slow:    the concat, its drop, `n = 0` → join
/// join:    the rest of the block
///
/// With no Str operand there is nothing to null-test and the lengths
/// are read in the head itself.
pub(super) fn fold_site(
    func: &mut Function,
    kernels: &Kernels,
    head: BlockId,
    at: usize,
    n: ValueId,
    left: (Operand, Kind),
    right: (Operand, Kind),
) {
    let fast = new_block(func);
    let join = split_block_at(func, head, &[at])[0];
    let slow = new_block(func);

    let concat = func.blocks[join.0 as usize].insts.remove(0);
    let origin = concat.origin;
    let r = concat.result.expect("a concat defines its result");
    let slow_block = &mut func.blocks[slow.0 as usize];
    slow_block.insts.push(concat);
    slow_block.insts.push(Inst {
        result: None,
        kind: InstKind::Call(kernels.str_drop, vec![Operand::Value(r)]),
        origin,
    });
    slow_block.insts.push(Inst {
        result: Some(n),
        kind: InstKind::Copy(Type::I64, Operand::ConstI64(0)),
        origin,
    });
    slow_block.term = Terminator::Br(join);

    let nulls: Vec<Operand> = [left, right]
        .into_iter()
        .filter(|&(_, kind)| kind == Kind::Str)
        .map(|(op, _)| {
            emit(
                func,
                head,
                Type::Bool,
                InstKind::ICmp(IPred::Eq, op, Operand::ConstPtrNull),
            )
        })
        .collect();
    let measure = match any_of(func, head, nulls) {
        Some(null) => {
            let measure = new_block(func);
            func.blocks[head.0 as usize].term = Terminator::CondBr {
                cond: null,
                then_blk: slow,
                else_blk: measure,
            };
            Some(measure)
        }
        None => None,
    };
    let test = measure.unwrap_or(head);
    let la = length(func, test, left);
    let lb = length(func, test, right);
    let sum = emit(func, test, Type::I64, InstKind::BinOp(BinOp::Add, la, lb));
    let over = emit(
        func,
        test,
        Type::Bool,
        InstKind::ICmp(IPred::Sgt, sum, Operand::ConstI64(STR_MAX_LEN)),
    );
    func.blocks[test.0 as usize].term = Terminator::CondBr {
        cond: over,
        then_blk: slow,
        else_blk: fast,
    };

    func.blocks[fast.0 as usize].insts.push(Inst {
        result: Some(n),
        kind: InstKind::Copy(Type::I64, sum),
        origin,
    });
    func.blocks[fast.0 as usize].term = Terminator::Br(join);

    // measure · fast · join follow the head; the slow arm stays last
    let appended = if measure.is_some() { 4 } else { 3 };
    let tail = &mut func.blocks[head.0 as usize + 1..];
    tail.rotate_right(appended);
    if measure.is_some() {
        tail[..4].rotate_left(3);
    }
    tail[appended - 1..].rotate_left(1);
    renumber(func);
}

/// `tests` ORed into one flag, read in `block`; `None` for no tests.
fn any_of(func: &mut Function, block: BlockId, tests: Vec<Operand>) -> Option<Operand> {
    let mut tests = tests.into_iter();
    let first = tests.next()?;
    Some(tests.fold(first, |acc, next| {
        let a = emit(func, block, Type::I64, InstKind::ZExtBoolToI64(acc));
        let b = emit(func, block, Type::I64, InstKind::ZExtBoolToI64(next));
        let or = emit(func, block, Type::I64, InstKind::BinOp(BinOp::Or, a, b));
        emit(
            func,
            block,
            Type::Bool,
            InstKind::ICmp(IPred::Ne, or, Operand::ConstI64(0)),
        )
    }))
}

/// The operand's length as an i64, read in `block`.
fn length(func: &mut Function, block: BlockId, (op, kind): (Operand, Kind)) -> Operand {
    match kind {
        Kind::Str => {
            let l = emit(
                func,
                block,
                Type::I32,
                InstKind::Load(Type::I32, op, STR_LEN_OFF),
            );
            emit(func, block, Type::I64, InstKind::ZExtI32ToI64(l))
        }
        Kind::Substr => emit(
            func,
            block,
            Type::I64,
            InstKind::Load(Type::I64, op, SUBSTR_LEN_OFF),
        ),
    }
}
