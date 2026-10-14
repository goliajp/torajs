//! The write side of a struct layout's own properties: where a write
//! through [[Set]] lands (`Object.assign`'s target). Moved out of
//! [`super`].

use crate::ast::PropKey;
use crate::ssa::{InstKind, Operand, SigId, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

/// Where a written own property's value GOES — the [[Set]] mirror of
/// [`super::PropSource`]. `Object.assign` (ES §20.1.2.1 step 4.c.ii.2) reaches
/// the TARGET through [[Set]], so the target's accessor half decides.
#[derive(Clone, Copy)]
pub(crate) enum PropSink {
    /// A plain data field at layout index `idx` — the slot takes the
    /// value's reference.
    Data { idx: usize, ty: Type },
    /// An accessor whose setter closure sits at layout index `idx`. The
    /// value is an ARG (borrowed, like every call arg), not a store —
    /// the checker already matched it against the setter's param type.
    Setter { idx: usize, sig: SigId },
    /// An accessor with a [[Get]] but no [[Set]] — writing it is a
    /// strict-mode `TypeError` (ES §10.1.9), which a module always is.
    GetterOnly,
}

/// One writable own property: the key, and where a write lands.
pub(crate) struct OwnPropSink {
    pub(crate) key: PropKey,
    pub(crate) sink: PropSink,
}

/// Walk a struct layout into its own properties as WRITE targets, in
/// declaration order. A get/set pair collapses into one entry keyed by
/// the plain name; unlike [`super::own_props`], the SETTER half wins — it is
/// the half a write goes through.
pub(crate) fn own_prop_sinks(layout: &[(PropKey, Type)]) -> Vec<OwnPropSink> {
    let mut out: Vec<OwnPropSink> = Vec::new();
    for (idx, (fname, fty)) in layout.iter().enumerate() {
        if crate::check_type_of_object_lit::private_slot(fname) {
            continue;
        }
        let (key, sink) = match crate::check_type_of_object_lit::accessor_slot(fname) {
            Some(("__setter_", prop)) => {
                let Type::Closure(sig) = *fty else {
                    panic!(
                        "ssa-lower: accessor slot `{}` is not a closure (got {fty:?})",
                        fname.lossy()
                    );
                };
                (PropKey::from(prop), PropSink::Setter { idx, sig })
            }
            Some((_, prop)) => (PropKey::from(prop), PropSink::GetterOnly),
            None => (fname.clone(), PropSink::Data { idx, ty: *fty }),
        };
        match out.iter_mut().find(|p| p.key == key) {
            // A getter seen first is superseded when its setter half
            // shows up — the write goes through the setter either way.
            Some(slot) if matches!(slot.sink, PropSink::GetterOnly) => slot.sink = sink,
            Some(_) => {}
            None => out.push(OwnPropSink { key, sink }),
        }
    }
    out
}

/// Write `value` into the property. `owned` says whether the value
/// arrives with its own reference (a getter's result) or borrowed out
/// of a source slot — a data slot takes ownership (so a borrowed value
/// is retained first), while a setter is a plain call arg (so an owned
/// value is released after it returns).
pub(crate) fn emit_prop_store(
    ctx: &mut LowerCtx<'_>,
    obj: &Operand,
    sink: &PropSink,
    value: Operand,
    owned: bool,
    val_ty: Type,
) {
    match *sink {
        PropSink::Data { idx, ty } => {
            let off = OBJ_HEADER_SIZE + (idx as u64) * 8;
            // Release what the slot held before overwriting it.
            if !ty.is_copy() {
                let cur = ctx.cur_block;
                let old = ctx
                    .f
                    .append_inst(cur, InstKind::Load(ty, *obj, off), ty, None);
                ctx.emit_drop_value(Operand::Value(old), ty);
            }
            if !owned && ty.is_refcounted() {
                ctx.emit_rc_inc(value);
            }
            // the width analysis joins the target field with the
            // source's, so an integer source may meet an f64 slot but
            // never the reverse
            let value = match (ty, val_ty) {
                (Type::F64, Type::I64) => ctx.coerce_to_f64(value),
                (Type::I64, Type::F64) => panic!(
                    "ssa-lower: f64 property into an i64 struct field — \
                     container width analysis missed this write"
                ),
                _ => value,
            };
            let cur = ctx.cur_block;
            ctx.f.append_void(cur, InstKind::Store(value, *obj, off));
        }
        PropSink::Setter { idx, sig } => {
            let off = OBJ_HEADER_SIZE + (idx as u64) * 8;
            crate::ssa_lower_call_struct_method_dispatch::emit_receiver_closure_call_ops(
                ctx,
                *obj,
                off,
                sig,
                vec![value],
            );
            ctx.emit_throw_check(None);
            if owned && val_ty.is_refcounted() {
                ctx.emit_drop_value(value, val_ty);
            }
        }
        PropSink::GetterOnly => {
            // The value was already read out of the source (ES orders
            // the [[Get]] before the [[Set]]); release it, then throw.
            if owned && val_ty.is_refcounted() {
                ctx.emit_drop_value(value, val_ty);
            }
            let cur = ctx.cur_block;
            ctx.f.append_void(
                cur,
                InstKind::Call(ctx.intrinsics.throw_readonly_assign, vec![]),
            );
            ctx.emit_throw_check(None);
        }
    }
}

/// Write the any-lane value `value` into the data property `key` of
/// `obj` through §10.1.9.2 [[Set]]: a value the slot can hold lands in
/// it, anything else stays in the field's sidecar behind the exotic
/// bit — what a source field a write of the program shadowed hands on
/// (`exotic_read::slot_value` answers it as an owned `any`).
pub(crate) fn emit_prop_store_any(
    ctx: &mut LowerCtx<'_>,
    obj: &Operand,
    key: &PropKey,
    value: Operand,
    owned: bool,
) {
    let strict = !crate::ssa_lower_set_flavor::cur_fn_is_sloppy(ctx);
    let (tag, val) = ctx.box_to_tag_value(value);
    let name_str = ctx.intern_string_literal(key.as_wtf8());
    let leg = ctx.intrinsics.obj_field_set_exotic;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            leg,
            vec![
                *obj,
                Operand::Value(name_str),
                tag,
                val,
                Operand::ConstI64(i64::from(strict)),
            ],
        ),
    );
    ctx.emit_throw_check(None);
    if owned {
        ctx.emit_drop_value(value, Type::Any);
    }
}
