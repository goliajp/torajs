//! T-27.b + T-29 — FnSig-as-Object + Array-as-Object Member read via
//! side-table-keyed-by-ptr props storage pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Member` god-arm
//! as chunk-65 of the decomp (chunks 1-64 = ... + typed-receiver
//! Member props cluster).
//!
//! Both arms share the same shape:
//!
//! 1. Intern the Member name as a `Str` literal.
//! 2. Call the `<kind>props_get_tag(obj, key)` intrinsic → I64.
//! 3. Call the `<kind>props_get_value(obj, key)` intrinsic → I64.
//! 4. `__torajs_any_box(tag, value) -> Any` wraps the (tag, value)
//!    pair into the unified `Any` representation. NULL or missing
//!    key folds to `ANY_UNDEF` inside the runtime.
//!
//! - **T-27.b** — `Type::FnSig(_)` receiver routes through
//!   `fnprops_get_tag` / `fnprops_get_value`.
//! - **T-29** — `Type::Arr(_)` receiver routes through
//!   `arrprops_get_tag` / `arrprops_get_value`.
//!
//! Both arms must run AFTER the typed-receiver cluster
//! ([`crate::ssa_lower_member_typed_props`]) so Arr `.length` /
//! FnSig `.length` / `.name` short-circuits fire first; otherwise
//! the props side table would shadow those compile-time folds.
//!
//! Returns `Some(op)` on hit; `None` on miss (receiver type not
//! `FnSig` or `Arr`).

use crate::ast::ExprId;
use crate::ssa::{FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    obj: ExprId,
    obj_val: Operand,
    obj_ty: Type,
    name: &str,
) -> Option<Operand> {
    let (tag_fid, val_fid) = match obj_ty {
        Type::FnSig(_) => (
            ctx.intrinsics.fnprops_get_tag,
            ctx.intrinsics.fnprops_get_value,
        ),
        Type::Arr(_) => {
            // A nullable-arr receiver (un-narrowed exec/match miss)
            // reaches the props intrinsics as NULL — guard arms a
            // catchable TypeError first (`m.groups` on a miss; RC-4
            // F1a shape, same as the `.length` short-circuit).
            crate::ssa_lower_nullable_guard::emit_nullable_arr_guard(ctx, obj, &obj_val);
            (
                ctx.intrinsics.arrprops_get_tag,
                ctx.intrinsics.arrprops_get_value,
            )
        }
        _ => return None,
    };
    Some(lower_props_read(ctx, obj_val, name, tag_fid, val_fid))
}

fn lower_props_read(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    name: &str,
    tag_fid: FuncId,
    val_fid: FuncId,
) -> Operand {
    let key_str = ctx.intern_string_literal(name);
    let cur_block = ctx.cur_block;
    let tag = ctx.f.append_inst(
        cur_block,
        InstKind::Call(tag_fid, vec![obj_val.clone(), Operand::Value(key_str)]),
        Type::I64,
        None,
    );
    let cur_block = ctx.cur_block;
    let value = ctx.f.append_inst(
        cur_block,
        InstKind::Call(val_fid, vec![obj_val, Operand::Value(key_str)]),
        Type::I64,
        None,
    );
    let cur_block = ctx.cur_block;
    let box_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(tag), Operand::Value(value)],
        ),
        Type::Any,
        None,
    );
    Operand::Value(box_v)
}
