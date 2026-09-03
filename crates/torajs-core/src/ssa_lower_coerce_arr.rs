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
//!
//! And the answer is not always the join. It is a WALK — §7.1.17
//! resolves `toString` on the receiver and §23.1.3.36 then resolves
//! `join` — which the kernel cannot see. A method call stands down
//! when the module might have touched either name; these three had no
//! callee for the checker to type, so they folded regardless and
//! answered "1,2" next to an `xs.toString()` that answered the patch.
//! Each now declines the same way, onto the any lane that runs the
//! real OrdinaryToPrimitive. A module that never names a builtin
//! prototype has an empty shadow set and emits exactly what it did.

use crate::ast::ExprId;
use crate::ssa::{ArrId, FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The `__torajs_arr_join_*` kernel for this array's element type —
/// the same dispatch `arr.toString()` takes in
/// `ssa_lower_str_arr_join_flat`.
///
/// `None` when no kernel answers this element type. The table used to
/// end in `_ => arr_join`, but `arr_join` is the Array<Str> kernel,
/// not a general one: it reads every slot as a `*Str` and asks it for
/// its units. A nested array's slot is a `*Arr`, a struct array's is a
/// `*Obj` — read that way they report length 0, so `String([[1],[2]])`
/// emitted the empty string and said nothing at all. Str is now an arm
/// of its own and everything else declines, onto the same any lane the
/// shadowed programs take, which runs the real per-element ToString.
fn join_fid(ctx: &LowerCtx<'_>, elem_arr_id: ArrId) -> Option<FuncId> {
    Some(match ctx.arr_layouts[elem_arr_id.0 as usize] {
        Type::Substr => ctx.intrinsics.arr_join_substr,
        Type::I64 => ctx.intrinsics.arr_join_i64,
        Type::F64 => ctx.intrinsics.arr_join_f64,
        Type::Bool => ctx.intrinsics.arr_join_bool,
        Type::Any => ctx.intrinsics.arr_join_any,
        Type::Str => ctx.intrinsics.arr_join,
        _ => return None,
    })
}

/// Does the typed side answer this array at all? The two reasons to
/// decline are one question at the call sites — a kernel that cannot
/// read these elements and a module that has patched the walk both
/// mean "hand it to the any lane" — so they are asked together.
fn kernel_join(ctx: &LowerCtx<'_>, elem_arr_id: ArrId) -> Option<FuncId> {
    if shadowed(ctx) {
        return None;
    }
    join_fid(ctx, elem_arr_id)
}

/// The join with the spec separator, leaving the caller its ownership
/// account. The result is a fresh owned Str.
fn emit_join_comma(ctx: &mut LowerCtx<'_>, arr: Operand, fid: FuncId) -> Operand {
    let sep = ctx.intern_string_literal(",");
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![arr, Operand::Value(sep)]),
        Type::Str,
        None,
    );
    // The any kernel is the one that runs user code per element, so
    // it is the one that can leave a throw pending (an own toString
    // that throws, or a Symbol element, which §7.1.17 step 2 rejects
    // by itself). The typed kernels read their slots and cannot.
    if fid == ctx.intrinsics.arr_join_any {
        ctx.emit_throw_check(None);
    }
    Operand::Value(v)
}

/// Has this module put anything in front of the walk the join kernel
/// stands in for?
fn shadowed(ctx: &LowerCtx<'_>) -> bool {
    crate::builtin_proto_shadow::arr_to_string_shadowed(&ctx.proto_shadow)
}

/// The one thing the any lane needs that the typed kernels do not: the
/// element kind, which the typed side carries in the SSA type and the
/// any side reads off the cell. A binding is marked where it is built,
/// but an array written INSIDE the expression (`[6] + ""`) has never
/// been handed to the any world before — its cell says nothing, and
/// the any join read it as empty. The locale lane in
/// `ssa_lower_str_arr_join_flat` marks ahead of its own any call for
/// exactly this.
fn stand_down_prelude(ctx: &mut LowerCtx<'_>, arr: &Operand) {
    ctx.emit_arr_mark_kind(arr);
}

/// S137 — `String(arr)` per ES §22.1.3.30 ToString of Array =
/// `arr.join(",")`.
///
/// `implicit_tostring` is the caller's own template-vs-`String()`
/// distinction, carried through unchanged: standing down takes the
/// Any arm those callers already have, kernel choice included, so a
/// shadowed array and an `any`-typed one answer through the same
/// runtime and cannot drift.
pub(crate) fn emit_to_string(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    elem_arr_id: ArrId,
    implicit_tostring: bool,
) -> Operand {
    let Some(join) = kernel_join(ctx, elem_arr_id) else {
        stand_down_prelude(ctx, &arg_op);
        let fid = if implicit_tostring {
            ctx.intrinsics.any_to_str_box
        } else {
            ctx.intrinsics.any_to_display_str
        };
        let boxed = ctx.box_to_any(arg_op.clone());
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(fid, vec![boxed]),
            Type::Str,
            None,
        );
        ctx.release_owned_temp(arg_eid, &arg_op);
        ctx.emit_throw_check(None);
        return Operand::Value(v);
    };
    let s = emit_join_comma(ctx, arg_op.clone(), join);
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
    let Some(join) = kernel_join(ctx, elem_arr_id) else {
        stand_down_prelude(ctx, &arg_op);
        let boxed = ctx.box_to_any(arg_op.clone());
        let v = Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.number_ctor_any, vec![boxed]),
            Type::F64,
            None,
        ));
        ctx.emit_throw_check(None);
        ctx.release_owned_temp(arg_eid, &arg_op);
        return v;
    };
    let s = emit_join_comma(ctx, arg_op.clone(), join);
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
    let Some(join) = kernel_join(ctx, elem_arr_id) else {
        stand_down_prelude(ctx, &v);
        // The Obj side's route: the any-lane heap dispatch (tag 4 =
        // the Heap slot tag; the header tag picks the kernel).
        let raw = ctx
            .f
            .append_inst(ctx.cur_block, InstKind::PtrToInt(v), Type::I64, None);
        let s = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_to_str_prim,
                vec![Operand::ConstI64(4), Operand::Value(raw)],
            ),
            Type::Str,
            None,
        );
        ctx.emit_throw_check(None);
        return (Operand::Value(s), true);
    };
    (emit_join_comma(ctx, v, join), true)
}
