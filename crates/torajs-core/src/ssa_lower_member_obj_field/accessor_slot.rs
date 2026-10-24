//! Accessor calls through a vtable slot.
//!
//! A subclass that redeclares an accessor pair overrides it exactly as
//! it would a method (§10.1.8.1 [[Get]] / §10.1.9.2 [[Set]] find the
//! subclass prototype's pair first), so each face of an overridden
//! pair takes a slot under its own symbol spelling, `<key>_get` /
//! `<key>_set` — the tail of the `__cm_<C>__<key>_get` body the slot
//! rows resolve to. A read or write through a receiver whose static
//! class has a redeclaring descendant loads the slot; one without such
//! a descendant keeps its direct call. The accessor walk has already
//! sent every site whose descendants the slot does not carry to the
//! any-member lane, so a slot found here covers them all.

use crate::ast::{ExprId, PropKey, mangle_key};
use crate::ssa::{FuncId, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// One face's slot: its index, its method-table name, its owners.
pub(crate) struct AccessorSlot {
    idx: u32,
    name: String,
    owners: Vec<String>,
}

/// The receiver's full class key (a generic instantiation keeps its
/// arguments) when `obj` is statically a user class instance — the
/// key the accessor walks start from.
pub(crate) fn class_key_of(ctx: &LowerCtx<'_>, obj: ExprId) -> Option<String> {
    match ctx.expr_types.get(&obj)? {
        crate::check::Type::ClassRef(n)
            if ctx
                .ast
                .class_parents
                .contains_key(n.split('<').next().unwrap_or(n.as_str())) =>
        {
            Some(n.clone())
        }
        _ => None,
    }
}

/// The slot a site must read when a strict descendant of the
/// receiver's static class redeclares the pair; `None` when the
/// resolved body is the only one an instance can be wearing, or when
/// the method-table name belongs to a regular method rather than an
/// accessor face.
pub(crate) fn find(
    ctx: &LowerCtx<'_>,
    cls_key: &str,
    prop: &str,
    face_suffix: &str,
) -> Option<AccessorSlot> {
    let name = format!("{}{face_suffix}", mangle_key(&PropKey::from(prop)));
    if !ctx.ast.accessor_slots.contains(&name) {
        return None;
    }
    let idx = ctx.ast.method_index.get(&name).copied()?;
    let owners = ctx.ast.method_owners.get(&name)?;
    let static_class = cls_key.split('<').next().unwrap_or(cls_key);
    if !crate::ssa_lower_call_vtable_dispatch::overridden_below(ctx, static_class, owners) {
        return None;
    }
    Some(AccessorSlot {
        idx,
        owners: owners.clone(),
        name,
    })
}

/// Call the face through the slot. `fid` is the body the static class
/// resolves to (its specialization for a generic declarer): every row
/// of the slot shares its signature once the slot joins have run.
/// `decl_fn` is that body's declared name, so the specialization's
/// `$$` suffix can be read off `fid`'s name for the may-throw lookup.
pub(crate) fn emit_call(
    ctx: &mut LowerCtx<'_>,
    slot: &AccessorSlot,
    recv: ExprId,
    fid: FuncId,
    decl_fn: &str,
    argv: Vec<Operand>,
) -> ValueId {
    let sig = ctx.fn_sig_ids[&fid];
    let ret_ty: Type = ctx.f_ret_type_hint(fid);
    let v =
        crate::ssa_lower_call_vtable_dispatch::emit_vtable_call(ctx, slot.idx, sig, ret_ty, argv);
    let fn_name = ctx.f_name_of(fid);
    let suffix = fn_name.strip_prefix(decl_fn).unwrap_or("");
    if crate::ssa_lower_call_vtable_dispatch::slot_may_throw(
        ctx,
        recv,
        &slot.name,
        suffix,
        &slot.owners,
    ) {
        ctx.emit_throw_check(None);
    } else {
        ctx.proofs.log_slot_call(&ctx.f.name);
    }
    v
}
