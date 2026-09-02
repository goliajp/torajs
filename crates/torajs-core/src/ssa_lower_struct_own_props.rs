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

use crate::ast::PropKey;
use crate::ssa::{InstKind, Operand, SigId, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

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

/// Walk a struct layout into its own properties, in declaration order.
/// A get/set pair collapses into one entry keyed by the plain name (the
/// getter half wins whichever order the two slots appear in).
pub(crate) fn own_props(layout: &[(PropKey, Type)], fn_sigs: &[(Vec<Type>, Type)]) -> Vec<OwnProp> {
    let mut out: Vec<OwnProp> = Vec::new();
    for (idx, (fname, fty)) in layout.iter().enumerate() {
        let (key, src) = match crate::check_type_of_object_lit::accessor_slot(fname) {
            Some(("__getter_", prop)) => {
                let Type::Closure(sig) = *fty else {
                    panic!("ssa-lower: accessor slot `{fname}` is not a closure (got {fty:?})");
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

/// Where a written own property's value GOES — the [[Set]] mirror of
/// [`PropSource`]. `Object.assign` (ES §20.1.2.1 step 4.c.ii.2) reaches
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
/// the plain name; unlike [`own_props`], the SETTER half wins — it is
/// the half a write goes through.
pub(crate) fn own_prop_sinks(layout: &[(PropKey, Type)]) -> Vec<OwnPropSink> {
    let mut out: Vec<OwnPropSink> = Vec::new();
    for (idx, (fname, fty)) in layout.iter().enumerate() {
        let (key, sink) = match crate::check_type_of_object_lit::accessor_slot(fname) {
            Some(("__setter_", prop)) => {
                let Type::Closure(sig) = *fty else {
                    panic!("ssa-lower: accessor slot `{fname}` is not a closure (got {fty:?})");
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
                    .append_inst(cur, InstKind::Load(ty, obj.clone(), off), ty, None);
                ctx.emit_drop_value(Operand::Value(old), ty);
            }
            if !owned && ty.is_refcounted() {
                ctx.emit_rc_inc(value.clone());
            }
            let cur = ctx.cur_block;
            ctx.f
                .append_void(cur, InstKind::Store(value, obj.clone(), off));
        }
        PropSink::Setter { idx, sig } => {
            let off = OBJ_HEADER_SIZE + (idx as u64) * 8;
            crate::ssa_lower_call_struct_method_dispatch::emit_receiver_closure_call_ops(
                ctx,
                obj.clone(),
                off,
                sig,
                vec![value.clone()],
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

/// Emit the property's value off `obj`. The bool is OWNED: `true` when
/// the value arrives with its own reference (a getter's result, a fresh
/// `undefined` box), `false` when it is borrowed out of the struct's
/// slot and the consumer must take its own share before storing it.
pub(crate) fn emit_prop_value(
    ctx: &mut LowerCtx<'_>,
    obj: &Operand,
    prop: &OwnProp,
) -> (Operand, bool) {
    match prop.src {
        PropSource::Data { idx, ty } => {
            let off = OBJ_HEADER_SIZE + (idx as u64) * 8;
            let cur = ctx.cur_block;
            let v = ctx
                .f
                .append_inst(cur, InstKind::Load(ty, obj.clone(), off), ty, None);
            (Operand::Value(v), false)
        }
        // Blade 1's `(__env, __this)` ABI — a getter is a zero-arg
        // method on the receiver. Its result is owned.
        PropSource::Getter { idx, sig, .. } => {
            let off = OBJ_HEADER_SIZE + (idx as u64) * 8;
            let v = crate::ssa_lower_call_struct_method_dispatch::emit_receiver_closure_call(
                ctx,
                obj.clone(),
                off,
                sig,
                &[],
            );
            ctx.emit_throw_check(None);
            (v, true)
        }
        PropSource::SetterOnly => {
            let cur = ctx.cur_block;
            let v = ctx.f.append_inst(
                cur,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            (Operand::Value(v), true)
        }
    }
}
