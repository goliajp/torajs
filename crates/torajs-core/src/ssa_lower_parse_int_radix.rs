//! §19.2.5.1 step 2 — ToInt32 of the `parseInt` radix, for both
//! spellings (`parseInt` and its §21.1.2.13 alias
//! `Number.parseInt`).
//!
//! The two call sites used to answer the step separately and neither
//! answered all of it: each branched on the shape it happened to be
//! handed, so a radix whose SSA type was not already number-shaped
//! reached `coerce_to_i64` and panicked. The spec asks for no shape
//! at all — ToInt32 runs ToNumber, which takes any value — so this
//! settles it in one place: anything that is not already an integer
//! slot rides the any-lane ToNumber and comes back through the same
//! `NaN → 0 / ±∞ → saturate` fold.
//!
//! The heap arm exists because a TS `as` cast is a value-layer
//! pass-through: `obj as any` keeps its `Obj` SSA type even though
//! the checker calls it Any, so the shape can be a struct, a string
//! slot or an array. Boxing is a pure encode over the operand, so
//! the temp release names the RAW operand and runs after the read.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Lower `args[idx]` as a radix. `arg` is the already-lowered
/// operand; `eid` is the expression it came from (for the owned-temp
/// release).
pub(crate) fn to_int32(ctx: &mut LowerCtx<'_>, eid: ExprId, arg: Operand) -> Operand {
    let ty = ctx.operand_ty(&arg);
    if ty == Type::I64 || ty == Type::I32 || matches!(arg, Operand::ConstI64(_)) {
        return arg;
    }
    if matches!(ty, Type::F64 | Type::Bool) {
        return ctx.coerce_to_i64(arg);
    }
    let boxed = if ty == Type::Any {
        arg.clone()
    } else {
        ctx.box_to_any(arg.clone())
    };
    let f = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_to_number, vec![boxed]),
        Type::F64,
        None,
    );
    if ty != Type::Any {
        ctx.release_owned_temp(eid, &arg);
    }
    // The coercion is a real ToNumber: a `valueOf` that throws
    // propagates, and an object whose valueOf AND toString both
    // answer objects raises TypeError (§7.1.1 OrdinaryToPrimitive
    // step 5). `any_to_number` records those on the throw TLS and
    // answers NaN; without the check the parse ran anyway and the
    // pending throw surfaced at an unrelated site (t262
    // `parseInt/S15.1.2.2_A3.1_T7` CHECK#7/#8 crashed the process
    // instead of throwing).
    ctx.emit_throw_check(None);
    ctx.coerce_to_i64(Operand::Value(f))
}
