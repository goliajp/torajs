//! How an ARRAY becomes a string or a number, for the three coercion
//! sites that ask.
//!
//! `String(xs)` and a template substitution reach it through
//! `ssa_lower_call_coercion::emit_to_string`, `xs + ""` through
//! `ssa_lower_binop_inner::add_str`, and `Number(xs)` through
//! `emit_to_number` — which is the string question again, because
//! §7.1.4 ToNumber(Array) is ToNumber(ToString(Array)).
//!
//! The three carried a verbatim copy each of the element-type join
//! table, which is one fact about the array's layout and belongs in
//! one place. The seam is not "which caller" but "what does an array
//! turn into": the callers keep the ownership account and the shape of
//! their own result (an Operand, a `(value, owned)` pair, an f64), and
//! this answers what to call.

use crate::ast::ExprId;
use crate::ssa::{ArrId, FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The `__torajs_arr_join_*` kernel for this array's element type —
/// the same dispatch `arr.toString()` takes in
/// `ssa_lower_str_arr_join_flat`.
fn join_fid(ctx: &LowerCtx<'_>, elem_arr_id: ArrId) -> FuncId {
    match ctx.arr_layouts[elem_arr_id.0 as usize] {
        Type::Substr => ctx.intrinsics.arr_join_substr,
        Type::I64 => ctx.intrinsics.arr_join_i64,
        Type::F64 => ctx.intrinsics.arr_join_f64,
        Type::Bool => ctx.intrinsics.arr_join_bool,
        Type::Any => ctx.intrinsics.arr_join_any,
        _ => ctx.intrinsics.arr_join,
    }
}

/// The join with the spec separator, leaving the caller its ownership
/// account. The result is a fresh owned Str.
fn emit_join_comma(ctx: &mut LowerCtx<'_>, arr: Operand, elem_arr_id: ArrId) -> Operand {
    let fid = join_fid(ctx, elem_arr_id);
    let sep = ctx.intern_string_literal(",");
    Operand::Value(ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![arr, Operand::Value(sep)]),
        Type::Str,
        None,
    ))
}

/// S137 — `String(arr)` per ES §22.1.3.30 ToString of Array =
/// `arr.join(",")`.
pub(crate) fn emit_to_string(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    elem_arr_id: ArrId,
) -> Operand {
    let s = emit_join_comma(ctx, arg_op.clone(), elem_arr_id);
    ctx.release_owned_temp(arg_eid, &arg_op);
    s
}

/// S172 — `Number(Array<T>)` per ES §7.1.4 ToNumber(Array) =
/// ToNumber(ToString(Array)) = ToNumber(arr.join(",")). The resulting
/// Str feeds str_to_number (NaN on a non-numeric join result).
pub(crate) fn emit_to_number(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    elem_arr_id: ArrId,
) -> Operand {
    let s = emit_join_comma(ctx, arg_op.clone(), elem_arr_id);
    ctx.release_owned_temp(arg_eid, &arg_op);
    let n = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.str_to_number, vec![s.clone()]),
        Type::F64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.str_drop, vec![s]),
    );
    Operand::Value(n)
}

/// S138 — the array side of a mixed `+` string concat, which reuses
/// the S137 dispatch. `true` is the caller's "this operand is a fresh
/// owned temp to drop".
pub(crate) fn emit_concat_side(
    ctx: &mut LowerCtx<'_>,
    v: Operand,
    elem_arr_id: ArrId,
) -> (Operand, bool) {
    (emit_join_comma(ctx, v, elem_arr_id), true)
}
