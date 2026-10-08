//! Member access through an interface that dispatches by witness slots
//! (`crate::iface_witness`): the receiver may be any class instance
//! that implements the interface or an object literal of its shape, so
//! a member is reached through the receiver's vtable row — a call face
//! for a method, a get / set face for a data member — never at the
//! interface's own field offset.
//!
//! The witness thunks that fill the rows (and the literal ones that
//! read a literal's own fields) are where the typed lanes run; inside a
//! literal thunk the receiver IS a literal of the interface's layout,
//! so its own member accesses take the plain field lanes.

use crate::ast::ExprId;
use crate::iface_witness::{get_face, lit_thunk, set_face};
use crate::ssa::{Operand, StructId, Type};
use crate::ssa_lower::LowerCtx;

mod reflect;
pub(crate) use reflect::{lower_reflect_arg, refuse_reflection, refuse_spread};

/// The interface whose witness slots a member access on `recv` goes
/// through: the receiver checks as the interface's own type. A class
/// instance checks as its class, so its members stay its own even where
/// its layout id is shared with an interface of the same shape.
pub(crate) fn receiver_iface(ctx: &LowerCtx<'_>, recv: ExprId) -> Option<String> {
    let slots = &ctx.ast.iface_slots;
    if slots.is_empty() || ctx.f.name.starts_with("__ilit_") {
        return None;
    }
    slots
        .iface_of(ctx.expr_types.get(&recv)?)
        .map(str::to_string)
}

/// Whether `sid` lays out an interface that dispatches through witness
/// slots (by member names: one interface type can intern more than one
/// layout id) — a value on it may be a class instance of another
/// layout.
pub(crate) fn sid_is_iface(ctx: &LowerCtx<'_>, sid: StructId) -> bool {
    let layout = &ctx.struct_layouts[sid.0 as usize];
    ctx.ast.iface_slots.layouts().any(|(_, names)| {
        names.len() == layout.len() && names.iter().zip(layout).all(|(n, (k, _))| *n == k)
    })
}

/// An interface-typed slot takes any object pointer the checker admitted
/// into it: a class instance or a literal, whatever its layout id.
pub(crate) fn slot_takes(ctx: &LowerCtx<'_>, slot: Type, v: Type) -> bool {
    matches!((slot, v), (Type::Obj(s), Type::Obj(_)) if sid_is_iface(ctx, s))
}

/// The interfaces whose members are a prefix of `layout`: the ones a
/// literal of that layout can stand in for, whose slots its row fills.
pub(crate) fn lit_row_ifaces(
    ast: &crate::ast::Ast,
    layout: &[(crate::ast::PropKey, Type)],
) -> Vec<String> {
    let mut out: Vec<String> = ast
        .iface_slots
        .layouts()
        .filter(|(_, names)| {
            names.len() <= layout.len() && names.iter().zip(layout).all(|(n, (k, _))| *n == k)
        })
        .map(|(i, _)| i.to_string())
        .collect();
    out.sort();
    out
}

/// The vtable row a literal of layout `sid` wears, if any interface
/// slot needs one.
pub(crate) fn lit_row_name(sid: StructId) -> String {
    format!("__iface_lit_{}", sid.0)
}

/// Emit the slot call for `face` of `iface` with the receiver first.
/// The row's signature is the literal thunk's: every row of the slot
/// shares it (see `num_width::slot_abi::iface_unions`). `args` are the
/// source expressions behind `argv[1..]`, for the argument coercions.
/// Answers the call's result.
pub(crate) fn emit_face_call(
    ctx: &mut LowerCtx<'_>,
    eid: Option<ExprId>,
    iface: &str,
    face: &str,
    mut argv: Vec<Operand>,
    args: &[ExprId],
) -> Option<Operand> {
    let base = ctx.ast.method_index.len();
    let slot = ctx.ast.iface_slots.slot_of(base, iface, face)?;
    let fid = *ctx.fn_table.get(&lit_thunk(iface, face))?;
    let sig = *ctx.fn_sig_ids.get(&fid)?;
    let ret_ty = ctx.f_ret_type_hint(fid);
    if let Some(eid) = eid {
        crate::ssa_lower_call_terminal::pad_trailing_undef(ctx, eid, &mut argv);
    }
    let param_tys = ctx.fn_sigs[sig.0 as usize].0.clone();
    let n = args.len().min(argv.len().saturating_sub(1));
    let coerced = crate::ssa_lower_call_terminal_coerce::coerce_args_by_param_tys(
        ctx,
        param_tys.get(1..).unwrap_or(&[]),
        &args[..n],
        &mut argv[1..=n],
    );
    let r = crate::ssa_lower_call_vtable_dispatch::emit_vtable_call(ctx, slot, sig, ret_ty, argv);
    if face_may_throw(ctx, iface, face) {
        ctx.emit_throw_check(None);
    }
    for (op, ty) in coerced {
        ctx.emit_drop_value(op, ty);
    }
    Some(Operand::Value(r))
}

fn face_may_throw(ctx: &LowerCtx<'_>, iface: &str, face: &str) -> bool {
    let slots = &ctx.ast.iface_slots;
    let lit = lit_thunk(iface, face);
    ctx.may_throw_fns.contains(&lit)
        || slots.witnesses.get(iface).is_some_and(|cs| {
            cs.iter().any(|c| {
                ctx.may_throw_fns
                    .contains(&crate::iface_witness::class_thunk(c, iface, face))
            })
        })
}

/// `recv.name` read through the get face. A method member read as a
/// value has no face to read it through.
pub(crate) fn try_get(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    obj_val: Operand,
    name: &str,
) -> Option<Operand> {
    let iface = receiver_iface(ctx, obj)?;
    let op = emit_face_call(ctx, None, &iface, &get_face(name), vec![obj_val], &[])?;
    // a get face is a call: its result arrives owned
    if !ctx.operand_ty(&op).is_copy() {
        ctx.owned_member_reads.insert(eid);
    }
    Some(op)
}

/// `recv.field = value` through the set face; evaluates to the value.
pub(crate) fn try_set(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    obj_val: Operand,
    field: &str,
    value: ExprId,
) -> Option<Operand> {
    let iface = receiver_iface(ctx, obj)?;
    let face = set_face(field);
    let base = ctx.ast.method_index.len();
    ctx.ast.iface_slots.slot_of(base, &iface, &face)?;
    let v = ctx.lower_expr(value);
    let transfers = ctx.expr_transfers_ownership(value);
    // SHARE: the face borrows the value; the field store inside the
    // thunk takes the field's own stake
    emit_face_call(ctx, None, &iface, &face, vec![obj_val, v], &[value])?;
    let v_ty = ctx.operand_ty(&v);
    Some(crate::ssa_lower_assign_member::finish_assign_value(
        ctx, eid, v, v_ty, transfers,
    ))
}
