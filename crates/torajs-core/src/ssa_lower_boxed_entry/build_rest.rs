//! The rest-slot half of the boxed adapter — collect
//! `argv[fixed..argc]` into an `Arr<Any>` for a rest parameter, and
//! convert it to the typed repr when the rest is typed.
//!
//! Moved verbatim out of `build.rs` (rotation 627) when that file
//! stood at 468 lines and the adapter build had to grow a receiver
//! lead for the argv-face static twin.

use crate::ssa::{self, FuncId, InstKind, Operand, Terminator, Type};

use super::BoxedEntryIntrinsics;
use super::unbox::BoxedCoerceIntrinsics;

/// The rest-slot half of the adapter (刀 2 extraction, function-size
/// line): collect argv[fixed..argc] into an Arr<Any>, convert to the
/// typed repr when the rest is typed (NULL-gating the catchable
/// mismatch TypeError into an early undefined return), and record
/// the value's box as a post-call temp. Answers the slot operand and
/// the (possibly new) current block.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_rest_slot(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    intr: &BoxedEntryIntrinsics,
    coerce: &BoxedCoerceIntrinsics<'_>,
    arr_any_to_typed: FuncId,
    argv: ssa::ValueId,
    argc: ssa::ValueId,
    n_fixed: usize,
    arr_ty: Type,
    rest_kind: Option<i64>,
    obj_temps: &mut Vec<ssa::ValueId>,
) -> (ssa::ValueId, ssa::BlockId) {
    let mut entry = entry;
    let any_arr = emit_rest_collect(f, entry, intr, argv, argc, n_fixed, arr_ty);
    // A TYPED rest converts the Arr<Any> collection through the
    // assign-boundary kernel (per-slot coerce + kind stamp). A
    // mismatched element arms the catchable TypeError and answers
    // NULL; the gate below short-circuits before the body could
    // read the NULL block, and the pending throw surfaces at the
    // caller's check point.
    let arr = match rest_kind {
        Some(kind) => {
            let typed = f.append_inst(
                entry,
                InstKind::Call(
                    arr_any_to_typed,
                    vec![Operand::Value(any_arr), Operand::ConstI64(kind)],
                ),
                arr_ty,
                None,
            );
            // Release the Any collection now that the typed copy
            // owns the elements' new stakes.
            let boxed_src = f.append_inst(
                entry,
                InstKind::Call(
                    intr.any_box,
                    vec![Operand::ConstI64(4), Operand::Value(any_arr)],
                ),
                Type::Any,
                None,
            );
            f.append_void(
                entry,
                InstKind::Call(coerce.ids.anyv_rc_dec, vec![Operand::Value(boxed_src)]),
            );
            let pi = f.append_inst(
                entry,
                InstKind::PtrToInt(Operand::Value(typed)),
                Type::I64,
                None,
            );
            let isnull = f.append_inst(
                entry,
                InstKind::ICmp(ssa::IPred::Eq, Operand::Value(pi), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            let throw_blk = f.add_block();
            let cont_blk = f.add_block();
            f.set_term(
                entry,
                Terminator::CondBr {
                    cond: Operand::Value(isnull),
                    then_blk: throw_blk,
                    else_blk: cont_blk,
                },
            );
            let undef = f.append_inst(
                throw_blk,
                InstKind::Call(
                    intr.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            f.set_term(throw_blk, Terminator::Ret(Some(Operand::Value(undef))));
            entry = cont_blk;
            typed
        }
        None => any_arr,
    };
    let boxed_arr = f.append_inst(
        entry,
        InstKind::Call(
            intr.any_box,
            vec![Operand::ConstI64(4), Operand::Value(arr)],
        ),
        Type::Any,
        None,
    );
    obj_temps.push(boxed_arr);
    (arr, entry)
}

fn emit_rest_collect(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    intr: &BoxedEntryIntrinsics,
    argv: ssa::ValueId,
    argc: ssa::ValueId,
    n_fixed: usize,
    arr_ty: Type,
) -> ssa::ValueId {
    let n_rest = f.append_inst(
        entry,
        InstKind::BinOp(
            ssa::BinOp::Sub,
            Operand::Value(argc),
            Operand::ConstI64(n_fixed as i64),
        ),
        Type::I64,
        None,
    );
    let sign = f.append_inst(
        entry,
        InstKind::BinOp(
            ssa::BinOp::AShr,
            Operand::Value(n_rest),
            Operand::ConstI64(63),
        ),
        Type::I64,
        None,
    );
    let not_sign = f.append_inst(
        entry,
        InstKind::BinOp(ssa::BinOp::Xor, Operand::Value(sign), Operand::ConstI64(-1)),
        Type::I64,
        None,
    );
    let count = f.append_inst(
        entry,
        InstKind::BinOp(
            ssa::BinOp::And,
            Operand::Value(n_rest),
            Operand::Value(not_sign),
        ),
        Type::I64,
        None,
    );
    let arr = f.append_inst(
        entry,
        InstKind::Call(intr.arr_alloc_any, vec![Operand::Value(count)]),
        arr_ty,
        None,
    );
    let pi = f.append_inst(
        entry,
        InstKind::PtrToInt(Operand::Value(argv)),
        Type::I64,
        None,
    );
    let off = f.append_inst(
        entry,
        InstKind::BinOp(
            ssa::BinOp::Add,
            Operand::Value(pi),
            Operand::ConstI64(8 * n_fixed as i64),
        ),
        Type::I64,
        None,
    );
    let pv = f.append_inst(
        entry,
        InstKind::IntToPtr(Operand::Value(off)),
        Type::Ptr,
        None,
    );
    f.append_void(
        entry,
        InstKind::Call(
            intr.arr_any_push,
            vec![
                Operand::Value(arr),
                Operand::Value(pv),
                Operand::Value(count),
                Operand::ConstPtrNull,
            ],
        ),
    );
    arr
}
