//! `p.value` Member reads: the parser spells `await e` as one
//! (`Ast::await_value_reads` marks it), and the lowering of a body that
//! suspends turns it into a suspension (`ssa_lower_fn::suspend`), using
//! the helpers here to pick the lane a settled promise's value is read
//! in. Every function that awaits suspends (`ast::desugar_async_ssa_gate`
//! moves each one), so an await read anywhere else is a compiler bug.
//!
//! **P10.4 identity** — an unmarked `.value` read (a desugar-rebuilt
//! Member that lost its mark) on a primitive whitelist (`check::Type`
//! Number / String / Boolean / Array(_) / BigInt) answers the operand
//! itself, as its await would (`Promise.resolve(e)` of a non-thenable
//! yields e). `Type::Object("...")` and `Type::Struct` are NOT in the
//! whitelist because they CAN carry a real user-defined `.value` field —
//! those fall through to the regular Member path.
//!
//! **Value-cast switch** — a settled cell's value word is an i64
//! whatever T is; [`cast_promise_value`] gives it the SSA shape:
//! - heap T (`Str / Substr / Obj / Arr / Closure / RegExp / Date /
//!   Symbol / Promise / Any / Ptr`): `IntToPtr` + `emit_rc_inc` (the
//!   promise owns one reference on its value, released with the cell)
//! - `Bool`: `TruncI64ToBool`
//! - `F64`: `BitCastI64ToF64` (resolve and the callback thunks write
//!   f64 bits)
//! - Other / None: the raw i64

use crate::ast::ExprId;
use crate::check::{self as check_mod};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_parse_type::parse_type;

mod detect;
mod undef_slot;
mod unmarked;

pub(crate) use detect::detect_obj_is_builtin_promise;
pub(crate) use undef_slot::{keep_undefined, undef_cell_slot};
use unmarked::try_p10_4_primitive_identity;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    name: &str,
) -> Option<Operand> {
    if name != "value" {
        return None;
    }
    if ctx.ast.await_value_reads.contains(&eid) {
        assert!(
            ctx.suspend_body,
            "internal error: an await in `{}`, which does not suspend",
            ctx.f.name
        );
        return Some(crate::ssa_lower_fn::suspend::lower_await(ctx, eid, obj));
    }
    try_p10_4_primitive_identity(ctx, obj)
}

/// The repr code `__torajs_promise_get_value_as` should convert into,
/// for the lane [`cast_promise_value`] is about to cast to. `0` means
/// "leave the slot alone" — the lane is unknown, so there is nothing
/// to convert towards.
///
/// An `any` lane asks for REPR_ANY rather than the slot verbatim: the
/// cast rc_incs what it gets as a NaN box, and a cell reached through
/// `Promise.resolve(<any>)`'s same-object pass-through carries the
/// SOURCE promise's repr, which is routinely a plain i64.
pub(crate) fn awaited_lane_repr(inner_ssa_ty: Option<&Type>) -> i64 {
    let Some(ty) = inner_ssa_ty else {
        return 0;
    };
    if matches!(ty, Type::Any) {
        return crate::ssa_lower_promise_repr_mark::REPR_ANY;
    }
    let as_f64 = matches!(ty, Type::F64);
    crate::ssa_lower_promise_repr_mark::promise_value_repr(ty, as_f64, false).unwrap_or(0)
}

/// `pub(crate)`: the `Promise.all` lowering runs the same recovery on
/// its own call expression, so the array it BUILDS and the `await` that
/// READS it name the same element lane (`ssa_lower_call_promise_static`).
pub(crate) fn recover_inner_ssa_ty(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Option<Type> {
    let ann = if let Some(check_mod::Type::Promise(inner)) = ctx.expr_types.get(&obj) {
        crate::check_type_to_ann::type_to_value_ann(inner)
    } else {
        return None;
    };
    Some(parse_type(
        Some(&ann),
        ctx.aliases,
        ctx.arr_layouts,
        ctx.fn_sigs,
        ctx.generic_struct_decls,
        ctx.struct_layouts,
        ctx.inst_memo,
    ))
}

pub(crate) fn cast_promise_value(
    ctx: &mut LowerCtx<'_>,
    raw_v: crate::ssa::ValueId,
    inner_ssa_ty: Option<Type>,
) -> crate::ssa::ValueId {
    let cur_block = ctx.cur_block;
    match inner_ssa_ty {
        Some(t)
            if matches!(
                t,
                Type::Str
                    | Type::Substr
                    | Type::Obj(_)
                    | Type::Arr(_)
                    | Type::Closure(_)
                    | Type::RegExp
                    | Type::Date
                    | Type::Symbol
                    | Type::Promise
                    | Type::Any
                    | Type::Ptr
            ) =>
        {
            let ptr = ctx.f.append_inst(
                cur_block,
                InstKind::IntToPtr(Operand::Value(raw_v)),
                t,
                None,
            );
            ctx.emit_rc_inc(Operand::Value(ptr));
            ptr
        }
        Some(Type::Bool) => ctx.f.append_inst(
            cur_block,
            InstKind::TruncI64ToBool(Operand::Value(raw_v)),
            Type::Bool,
            None,
        ),
        Some(Type::F64) => ctx.f.append_inst(
            cur_block,
            InstKind::BitCastI64ToF64(Operand::Value(raw_v)),
            Type::F64,
            None,
        ),
        _ => raw_v,
    }
}
