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
use crate::ssa::{InstKind, Operand, StructId, Type};
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
    // an optional chain or a narrowing has tested a nullable receiver
    // by the time its member is reached
    let ty = match ctx.expr_types.get(&recv)? {
        crate::check::Type::Nullable(t) => t,
        t => t,
    };
    let iface = slots.iface_of(ty)?;
    (!holds_own_literal(ctx, recv)).then(|| iface.to_string())
}

/// Whether `recv` reads a binding that holds the object literal it was
/// declared with and nothing else, laid out in the declared type's
/// order: one the escape analysis let live on the stack (it keeps
/// reassigned and aliased bindings off it) whose slot every store of
/// this function so far fills with a stack allocation. Its members are read
/// in place, as before interfaces had slots.
fn holds_own_literal(ctx: &LowerCtx<'_>, recv: ExprId) -> bool {
    let crate::ast::Expr::Ident(n) = ctx.ast.get_expr(recv) else {
        return false;
    };
    let Some(info) = ctx.locals.get(n) else {
        return false;
    };
    if !ctx.stack_alloced_locals.contains(n) || info.boxed {
        return false;
    }
    let insts = || ctx.f.blocks.iter().flat_map(|b| b.insts.iter());
    let stack: std::collections::HashSet<_> = insts()
        .filter(|i| matches!(i.kind, InstKind::AllocaBytes(_)))
        .filter_map(|i| i.result)
        .collect();
    let stores: Vec<Operand> = insts()
        .filter_map(|i| match &i.kind {
            InstKind::Store(v, Operand::Value(at), _) if *at == info.slot => Some(*v),
            _ => None,
        })
        .collect();
    // the entry block's null store is the slot's state before the
    // declaration runs
    stores.iter().any(|v| matches!(v, Operand::Value(_)))
        && stores.iter().all(|v| match v {
            Operand::Value(x) => stack.contains(x),
            Operand::ConstPtrNull => true,
            _ => false,
        })
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

/// Store the vtable pointer of a struct of layout `sid` built outside
/// the object-literal lane (a typed `JSON.parse`, an iterator result,
/// `Object.fromEntries`): the literal row of its layout when one
/// exists, else null — such a value may be read through an interface
/// as well.
pub(crate) fn store_lit_vtable(ctx: &mut LowerCtx<'_>, obj: crate::ssa::ValueId, sid: StructId) {
    let layout = &ctx.struct_layouts[sid.0 as usize];
    let row = (!ctx.ast.iface_slots.is_empty() && !lit_row_ifaces(ctx.ast, layout).is_empty())
        .then(|| format!("__vtable_{}", lit_row_name(sid)));
    let vt = match row {
        Some(g) => {
            let g = ctx
                .f
                .append_inst(ctx.cur_block, InstKind::GlobalRef(g), Type::Ptr, None);
            Operand::Value(g)
        }
        None => Operand::ConstPtrNull,
    };
    let at = crate::ssa_lower::OBJ_VTABLE_OFF;
    ctx.f
        .append_void(ctx.cur_block, InstKind::Store(vt, Operand::Value(obj), at));
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
    } else {
        ctx.proofs.log_slot_call(&ctx.f.name);
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
