//! Reification arms (`__torajs_static_method_reify` /
//! `__torajs_static_field_reify` / `__torajs_class_accessor_reify` /
//! `__torajs_class_static_accessor_reify`) split out of
//! [`crate::ssa_lower_call_class_synth`] (file-size limit): each
//! resolves a class-synthesis sentinel into the runtime define call
//! that lands a real own entry on the class object / prototype.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Knife B cut 2 (RFC 20260717-class-first-class-value) —
/// `__torajs_static_method_reify("<C>", "<M>")`: resolve
/// `__sm_<C>__<M>`'s boxed adapter and hand the runtime the
/// `(tag, name-Str, adapter-vaddr)` triple so the class object gets
/// its own `<M>` function entry. An adapter-synthesis dropout
/// (unboxable signature) skips the define — the member read keeps
/// its current answer instead of minting an uncallable cell.
pub(super) fn try_lower_static_method_reify(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let Expr::String(mname) = ctx.ast.get_expr(args[1]) else {
        return None;
    };
    let cname = cname.clone();
    let mname = mname.clone();
    let Some(tag) = ctx.class_name_to_tag.get(&cname).copied() else {
        return Some(Operand::ConstI64(0));
    };
    let body = format!("__sm_{cname}__{mname}");
    let Some(&body_fid) = ctx.fn_table.get(body.as_str()) else {
        return Some(Operand::ConstI64(0));
    };
    let Some(&(adapter_fid, adapter_sig)) = ctx.boxed_entries.get(&body_fid) else {
        return Some(Operand::ConstI64(0));
    };
    let name_op = ctx.lower_expr(args[1]);
    let cur_block = ctx.cur_block;
    let adapter = ctx.f.append_inst(
        cur_block,
        InstKind::FnAddr(adapter_fid),
        Type::FnSig(adapter_sig),
        None,
    );
    let define = ctx.intrinsics.static_method_define;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            define,
            vec![
                Operand::ConstI64(tag as i64),
                name_op.clone(),
                Operand::Value(adapter),
            ],
        ),
    );
    // The lowered Str literal is a caller-owned temp — the runtime
    // key copy is the define's own (dynobj_define rc_incs the key).
    let ty = ctx.operand_ty(&name_op);
    ctx.emit_drop_value(name_op, ty);
    Some(Operand::ConstI64(0))
}

/// L3b static-field-reflect (2026-07-22) —
/// `__torajs_static_field_reify("<C>", "<f>", __sf_<C>__<f>)`: box
/// the global slot's current value to its `(tag, value)` pair (the
/// heap arm's rc_inc is the entry's stake — the define takes it)
/// and hand runtime the quad for a real own data entry on the class
/// object. A tag-miss (generator-factory class) skips the define.
pub(super) fn try_lower_static_field_reify(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() != 3 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let Some(tag) = ctx.class_name_to_tag.get(cname.as_str()).copied() else {
        return Some(Operand::ConstI64(0));
    };
    let name_op = ctx.lower_expr(args[1]);
    let val = ctx.lower_expr(args[2]);
    let (vtag, vvalue) = ctx.box_to_tag_value(val);
    let define = ctx.intrinsics.static_field_define;
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            define,
            vec![Operand::ConstI64(tag as i64), name_op.clone(), vtag, vvalue],
        ),
    );
    // The lowered Str literal is a caller-owned temp — the runtime
    // key copy is the define's own (dynobj_define rc_incs the key).
    let ty = ctx.operand_ty(&name_op);
    ctx.emit_drop_value(name_op, ty);
    Some(Operand::ConstI64(0))
}

/// RFC 20260718-accessor-reify 刀 2+3 —
/// `__torajs_class_accessor_reify("<C>", "<p>")` (instance,
/// `__cm_` faces onto the prototype) and its static twin
/// (`__sm_` faces onto the class object): resolves the face
/// bodies' boxed adapters (either may be absent for a get-/set-only
/// accessor) and hands runtime the `(tag, name-Str, get-vaddr,
/// set-vaddr)` quad. Both-adapters dropout skips the define.
pub(super) fn try_lower_class_accessor_reify(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
    is_static: bool,
) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let Expr::String(pname) = ctx.ast.get_expr(args[1]) else {
        return None;
    };
    let cname = cname.clone();
    let pname = pname.clone();
    let Some(tag) = ctx.class_name_to_tag.get(&cname).copied() else {
        return Some(Operand::ConstI64(0));
    };
    let body_prefix = if is_static { "__sm_" } else { "__cm_" };
    let face = |suffix: &str| -> Option<(crate::ssa::FuncId, crate::ssa::SigId)> {
        let body = format!("{body_prefix}{cname}__{pname}{suffix}");
        let body_fid = ctx.fn_table.get(body.as_str()).copied()?;
        ctx.boxed_entries.get(&body_fid).copied()
    };
    let get = face("_get");
    let set = face("_set");
    if get.is_none() && set.is_none() {
        return Some(Operand::ConstI64(0));
    }
    let cur_block = ctx.cur_block;
    let mut vaddr = |f: Option<(crate::ssa::FuncId, crate::ssa::SigId)>| match f {
        Some((fid, sig)) => Operand::Value(ctx.f.append_inst(
            cur_block,
            InstKind::FnAddr(fid),
            Type::FnSig(sig),
            None,
        )),
        None => Operand::ConstI64(0),
    };
    let get_op = vaddr(get);
    let set_op = vaddr(set);
    let name_op = ctx.lower_expr(args[1]);
    let define = if is_static {
        ctx.intrinsics.class_static_accessor_define
    } else {
        ctx.intrinsics.class_accessor_define
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            define,
            vec![
                Operand::ConstI64(tag as i64),
                name_op.clone(),
                get_op,
                set_op,
            ],
        ),
    );
    // The lowered Str literal is a caller-owned temp — the runtime
    // key copy is the define's own (dynobj_define rc_incs the key).
    let ty = ctx.operand_ty(&name_op);
    ctx.emit_drop_value(name_op, ty);
    Some(Operand::ConstI64(0))
}
