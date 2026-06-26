//! P8.2 accessor read + struct-field-layout fallback Member-access
//! arm for `Type::Obj(sid)` receivers pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Member` god-arm
//! as chunk-66 of the decomp (chunks 1-65 = ... + FnSig/Arr-as-
//! Object props-read cluster).
//!
//! This sibling is the **terminal Member-arm path**: by the time
//! lower_expr_inner reaches it, all earlier sibling chains have
//! returned `None` and `obj_ty` is expected to be
//! `Type::Obj(sid)`. The caller panics on any other obj_ty before
//! invoking the sibling, so `try_lower` assumes a struct receiver.
//!
//! Two sub-arms tried in source order:
//!
//! - **P8.2** — accessor read: `c.value` where C declares
//!   `get value(): T`. `desugar_classes` renamed the getter's
//!   FnDecl to `__cm_<C>__<name>_get` and recorded `(C, name) →
//!   fn_name` in `ast.accessor_getters`. Reverse-lookup obj's
//!   class from the struct's sid via the aliases table (same
//!   idiom as the Phase I.1 method-dispatch path); on hit, emit
//!   a Call to the getter and propagate any throw via
//!   `emit_throw_check(Some(fid))`. Bypasses the struct-layout
//!   field lookup (the accessor name is not a real field).
//! - **Struct-field-layout fallback** — walk
//!   `struct_layouts[sid]` for a field matching `name`, compute
//!   the offset (`OBJ_HEADER_SIZE + idx*8`), and emit a
//!   `Load(field_ty, obj_val, offset)`. Panics if the field is
//!   absent (typechecker upstream rejects).

use crate::ssa::{InstKind, Operand, StructId, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    sid: StructId,
    name: &str,
) -> Operand {
    if let Some(op) = try_accessor_getter(ctx, obj_val, sid, name) {
        return op;
    }
    lower_struct_field(ctx, obj_val, sid, name)
}

fn try_accessor_getter(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    sid: StructId,
    name: &str,
) -> Option<Operand> {
    let cname = reverse_lookup_class_name(ctx, sid)?;
    let getter_fn = ctx
        .ast
        .accessor_getters
        .get(&(cname, name.to_string()))
        .cloned()?;
    let fid = ctx.fn_table.get(&getter_fn).copied()?;
    let ret_ty = ctx.f_ret_type_hint(fid);
    let cur_block = ctx.cur_block;
    let v = ctx
        .f
        .append_inst(cur_block, InstKind::Call(fid, vec![obj_val]), ret_ty, None);
    ctx.emit_throw_check(Some(fid));
    Some(Operand::Value(v))
}

fn reverse_lookup_class_name(ctx: &LowerCtx<'_>, sid: StructId) -> Option<String> {
    for (n, ty) in ctx.aliases.iter() {
        if matches!(ty, Type::Obj(s) if s.0 == sid.0) && ctx.ast.class_parents.contains_key(n) {
            return Some(n.clone());
        }
    }
    None
}

fn lower_struct_field(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    sid: StructId,
    name: &str,
) -> Operand {
    let layout = &ctx.struct_layouts[sid.0 as usize];
    let (idx, field_ty) = layout
        .iter()
        .enumerate()
        .find_map(|(i, (fname, fty))| if fname == name { Some((i, *fty)) } else { None })
        .unwrap_or_else(|| {
            panic!("ssa-lower: struct {sid:?} has no field `{name}` (layout: {layout:?})")
        });
    let offset = OBJ_HEADER_SIZE + idx as u64 * 8;
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(field_ty, obj_val, offset),
        field_ty,
        None,
    );
    Operand::Value(v)
}
