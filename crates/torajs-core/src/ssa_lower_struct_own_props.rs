//! RFC 20260714-objlit-accessor blade 6 — the OWN PROPERTIES a struct
//! layout enumerates, as the compile-time reflection unfolds
//! (`Object.values` / `Object.entries`) need to see them.
//!
//! A layout slot is not a property:
//!
//! * `__getter_v` / `__setter_v` are two slots but ONE property `v`
//!   (ES §10.4), and its value is what the GETTER answers — reading the
//!   slot hands over the closure instead (`Object.entries` used to emit
//!   `["__getter_v", [Function: __getter_v]]` where bun says `["v", 2]`).
//! * a set-only property reads `undefined` (ES §10.1.8 — an accessor
//!   with no [[Get]]), which only exists as an `Any`.
//!
//! The getter's result is OWNED (a call answers +1), unlike the
//! borrowed field load next to it — [`emit_prop_value`] reports which,
//! so each consumer keeps its refcount ledger straight.

use crate::ast::{ExprId, PropKey};
use crate::ssa::{InstKind, Operand, SigId, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

mod sink;
pub(crate) use sink::{PropSink, emit_prop_store, emit_prop_store_any, own_prop_sinks};

/// Where an own property's value comes from.
pub(crate) enum PropSource {
    /// A plain data field at layout index `idx`.
    Data { idx: usize, ty: Type },
    /// An accessor whose getter closure sits at layout index `idx`.
    Getter { idx: usize, sig: SigId, ret: Type },
    /// An accessor with only a setter half — reads `undefined`.
    SetterOnly,
}

/// One own property: the key ES enumerates it under, and its source.
pub(crate) struct OwnProp {
    pub(crate) key: PropKey,
    pub(crate) src: PropSource,
}

impl OwnProp {
    /// The SSA type the property's VALUE has. A set-only property's
    /// `undefined` has no typed representation — it is an `Any`.
    pub(crate) fn ty(&self) -> Type {
        match self.src {
            PropSource::Data { ty, .. } => ty,
            PropSource::Getter { ret, .. } => ret,
            PropSource::SetterOnly => Type::Any,
        }
    }
}

/// Whether a data member of `props` is one a write of the program can
/// shadow (`shadow_read`): its slot may stop telling the member's value,
/// or whether it is still an own enumerable member, once a getter or a
/// write through an `any` receiver ran. A whole-object reader then
/// reads the receiver through the runtime own-walk, which asks each
/// member its live state when the walk reaches it.
pub(crate) fn some_member_shadowed(ctx: &LowerCtx<'_>, props: &[OwnProp], recv: ExprId) -> bool {
    use crate::ssa_lower_member_obj_field::shadow_read;
    getter_may_delete(ctx, props)
        || props.iter().any(|p| {
            matches!(p.src, PropSource::Data { ty, .. }
                if shadow_read::applies(ctx, Some(recv), &p.key.lossy(), ty)
                    || shadow_read::mistyped_field(ctx, &p.key.lossy(), ty))
        })
}

/// Whether a getter of `props` can delete a member a whole-object
/// reader has yet to reach: the layout has a getter and the program
/// can delete a property. §20.1.2.22 EnumerableOwnProperties and
/// §25.5.2.4 SerializeJSONObject take the key list first and read
/// each key when they reach it, so a member the getter deleted is
/// skipped — whatever its type, an accessor included, which the shadow
/// census (scalar data members) does not see.
pub(crate) fn getter_may_delete(ctx: &LowerCtx<'_>, props: &[OwnProp]) -> bool {
    ctx.ast.accessor_field_names.may_freeze_or_delete()
        && props
            .iter()
            .any(|p| matches!(p.src, PropSource::Getter { .. }))
}

/// [`some_member_shadowed`] for a receiver of type `ty`; false unless
/// it is a struct.
pub(crate) fn obj_member_shadowed(ctx: &LowerCtx<'_>, ty: &Type, recv: ExprId) -> bool {
    let Type::Obj(sid) = ty else {
        return false;
    };
    let props = own_props(&ctx.struct_layouts[sid.0 as usize], ctx.fn_sigs);
    some_member_shadowed(ctx, &props, recv)
}

/// Walk a struct layout into its own properties, in declaration order.
/// A get/set pair collapses into one entry keyed by the plain name (the
/// getter half wins whichever order the two slots appear in).
pub(crate) fn own_props(layout: &[(PropKey, Type)], fn_sigs: &[(Vec<Type>, Type)]) -> Vec<OwnProp> {
    let mut out: Vec<OwnProp> = Vec::new();
    for (idx, (fname, fty)) in layout.iter().enumerate() {
        if crate::check_type_of_object_lit::private_slot(fname) {
            continue;
        }
        let (key, src) = match crate::check_type_of_object_lit::accessor_slot(fname) {
            Some(("__getter_", prop)) => {
                let Type::Closure(sig) = *fty else {
                    panic!(
                        "ssa-lower: accessor slot `{}` is not a closure (got {fty:?})",
                        fname.lossy()
                    );
                };
                let ret = fn_sigs[sig.0 as usize].1;
                (PropKey::from(prop), PropSource::Getter { idx, sig, ret })
            }
            Some((_, prop)) => (PropKey::from(prop), PropSource::SetterOnly),
            None => (fname.clone(), PropSource::Data { idx, ty: *fty }),
        };
        match out.iter_mut().find(|p| p.key == key) {
            // The getter half carries the value; a lone setter seen
            // first is upgraded when its pair shows up.
            Some(slot) if matches!(slot.src, PropSource::SetterOnly) => slot.src = src,
            Some(_) => {}
            None => out.push(OwnProp { key, src }),
        }
    }
    out
}

/// Emit the property's value off `obj`. The bool is OWNED: `true` when
/// the value arrives with its own reference (a getter's result, a fresh
/// `undefined` box), `false` when it is borrowed out of the struct's
/// slot and the consumer must take its own share before storing it.
/// `src` is the expression `obj` is the value of, when there is one
/// (its class names the annotation a getter's answer is checked
/// against).
pub(crate) fn emit_prop_value(
    ctx: &mut LowerCtx<'_>,
    obj: &Operand,
    src: Option<ExprId>,
    prop: &OwnProp,
) -> (Operand, Type, bool) {
    match prop.src {
        PropSource::Data { idx, ty } => {
            let off = OBJ_HEADER_SIZE + (idx as u64) * 8;
            crate::ssa_lower_member_obj_field::exotic_read::slot_value(
                ctx,
                src,
                *obj,
                (off, ty),
                &prop.key.lossy(),
            )
        }
        // Blade 1's `(__env, __this)` ABI — a getter is a zero-arg
        // method on the receiver. Its result is owned.
        PropSource::Getter { idx, sig, .. } => {
            let off = OBJ_HEADER_SIZE + (idx as u64) * 8;
            let v = crate::ssa_lower_call_struct_method_dispatch::emit_receiver_closure_call(
                ctx,
                *obj,
                off,
                sig,
                &[],
            );
            ctx.emit_throw_check(None);
            (v, prop.ty(), true)
        }
        PropSource::SetterOnly => {
            let cur = ctx.cur_block;
            let v = ctx.f.append_inst(
                cur,
                InstKind::BoxAny(5, Operand::ConstI64(0)),
                Type::Any,
                None,
            );
            (Operand::Value(v), Type::Any, true)
        }
    }
}

/// A data property read for a consumer that takes any value as it is
/// (a key-value pair): the getter's answer, owned and boxed, when the
/// field can be an accessor; `None` when [`emit_prop_value`] answers.
pub(crate) fn emit_prop_value_any(
    ctx: &mut LowerCtx<'_>,
    obj: &Operand,
    prop: &OwnProp,
) -> Option<Operand> {
    let PropSource::Data { idx, ty } = prop.src else {
        return None;
    };
    let off = OBJ_HEADER_SIZE + (idx as u64) * 8;
    let name = prop.key.lossy();
    crate::ssa_lower_member_obj_field::exotic_read::any_field_value(ctx, *obj, (off, ty), &name)
}
