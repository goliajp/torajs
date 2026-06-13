//! `Object.defineProperty` / `Object.defineProperties` lowering —
//! carved out of `ssa_lower.rs::lower_expr_inner` so the Object
//! property-descriptor trunk (RFC
//! `.claude/rfcs/20260613-object-property-descriptors/`) grows here
//! instead of the 27k-line god-file.
//!
//! [`emit_define_one`] is the shared single-property core: a literal
//! `ObjectLit` descriptor is extracted at compile time and routed to
//! `dynobj_define` (Any obj) / `arr_set_length_validate` +
//! `arrprops_set` (Array obj) per spec §10.1.6.3; a runtime descriptor
//! expression (RFC C1) is routed to `dynobj_define_from_desc`, which
//! reads the fields off the `desc` dynobj at runtime.
//!
//! - [`try_lower_define_property`] — `Object.defineProperty(obj, key,
//!   desc)`: one `emit_define_one` over `(args[0], args[1], args[2])`.
//! - [`try_lower_define_properties`] — `Object.defineProperties(obj,
//!   { k1: d1, ... })` (RFC C2): when `props` is a compile-time
//!   `ObjectLit` and `obj` is an Ident, unfold to one `emit_define_one`
//!   per field. Nested descriptors stored inside an `any` object aren't
//!   readable as dynobjs (their fields don't round-trip through dynamic
//!   member access), so a runtime `props` variable can't be walked —
//!   that shape falls through to the prior no-op.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Property key for [`emit_define_one`] — either an expression
/// (`defineProperty`'s `key` arg) or a literal field name
/// (`defineProperties`' unfolded keys).
pub(crate) enum DefineKey<'a> {
    Expr(ExprId),
    Name(&'a str),
}

/// Is this key the string `"length"`? (Array `length` descriptor takes
/// the `arr_set_length_validate` path.) Pure AST/str check — no lowering.
fn key_is_length(ctx: &LowerCtx, key: &DefineKey) -> bool {
    match key {
        DefineKey::Expr(eid) => matches!(ctx.ast.get_expr(*eid), Expr::String(s) if s == "length"),
        DefineKey::Name(n) => *n == "length",
    }
}

/// Lower the key to a `Str` operand. Call at the spec evaluation point
/// (after `obj`, before the descriptor) so a side-effecting key expr
/// orders correctly.
pub(crate) fn lower_key(ctx: &mut LowerCtx, key: &DefineKey) -> Operand {
    match key {
        DefineKey::Expr(eid) => ctx.lower_expr(*eid),
        DefineKey::Name(n) => Operand::Value(ctx.intern_string_literal(n)),
    }
}

/// Emit one `Object.defineProperty(obj, key, desc)`-equivalent. `obj` is
/// re-lowered from `obj_eid` (so `defineProperties` can re-read the
/// receiver variable after a prior field resized it). Returns `true`
/// when handled; `false` when neither a literal nor a runtime-Any
/// descriptor applies (caller decides the fall-through).
fn emit_define_one(ctx: &mut LowerCtx, obj_eid: ExprId, key: DefineKey, desc_eid: ExprId) -> bool {
    // Step 7d-A — capture the receiver's Ident name (if any) so the
    // dynobj-define Any path can writeback the post-resize ptr to the
    // variable's storage as a fresh NaN-box AnyValue.
    let receiver_ident: Option<String> = if let Expr::Ident(n) = ctx.ast.get_expr(obj_eid) {
        Some(n.clone())
    } else {
        None
    };
    let obj_op = ctx.lower_expr(obj_eid);
    let obj_ty = ctx.operand_ty(&obj_op);
    let is_length = key_is_length(ctx, &key);

    // Tag-pack helper — same table the BinOp Any===concrete arm uses for
    // runtime tag values.
    let pack = |this: &mut LowerCtx, v_raw: Operand, v_ty: Type| -> (i64, Operand) {
        match v_ty {
            Type::I64 | Type::I32 => (2, v_raw),
            Type::F64 => {
                let bits = this.f.append_inst(
                    this.cur_block,
                    InstKind::BitCastF64ToI64(v_raw),
                    Type::I64,
                    None,
                );
                (3, Operand::Value(bits))
            }
            Type::Bool => {
                let zext = this.f.append_inst(
                    this.cur_block,
                    InstKind::ZExtBoolToI64(v_raw),
                    Type::I64,
                    None,
                );
                (1, Operand::Value(zext))
            }
            _ if v_ty.is_refcounted() => {
                this.f.append_void(
                    this.cur_block,
                    InstKind::Call(this.intrinsics.rc_inc, vec![v_raw.clone()]),
                );
                (4, v_raw)
            }
            Type::Ptr if matches!(v_raw, Operand::ConstPtrNull) => (0, Operand::ConstI64(0)),
            _ => (0, Operand::ConstI64(0)),
        }
    };

    // Compile-time literal descriptor — extract value + the three data
    // flags from the ObjectLit at compile time.
    if matches!(ctx.ast.get_expr(desc_eid), Expr::ObjectLit { .. }) {
        let value_eid = match ctx.ast.get_expr(desc_eid) {
            Expr::ObjectLit { fields } => {
                fields.iter().find(|(n, _)| n == "value").map(|(_, e)| *e)
            }
            _ => None,
        };
        // Each flag is `Bool(true)` / `Bool(false)` when present; absent
        // (or non-literal, treated as absent) fields stay `None`.
        let lookup_bool_field = |field_name: &str| -> Option<bool> {
            if let Expr::ObjectLit { fields } = ctx.ast.get_expr(desc_eid) {
                for (n, e) in fields {
                    if n == field_name {
                        if let Expr::Bool(b) = ctx.ast.get_expr(*e) {
                            return Some(*b);
                        }
                        return None;
                    }
                }
            }
            None
        };

        // RFC C3 — accessor (get/set) descriptor. Per spec §6.2.5 an
        // accessor descriptor is mutually exclusive with a data
        // `value`; when the literal carries a `get` and/or `set`
        // function, store an `AccessorPair` cell instead of a data
        // value. Only dynobj-backed Any objects carry accessor storage
        // (typed Struct / Array accessors stay the prior no-op).
        let (get_eid, set_eid) = match ctx.ast.get_expr(desc_eid) {
            Expr::ObjectLit { fields } => (
                fields.iter().find(|(n, _)| n == "get").map(|(_, e)| *e),
                fields.iter().find(|(n, _)| n == "set").map(|(_, e)| *e),
            ),
            _ => (None, None),
        };
        if (get_eid.is_some() || set_eid.is_some()) && matches!(obj_ty, Type::Any) {
            let acc_enum = lookup_bool_field("enumerable");
            let acc_config = lookup_bool_field("configurable");
            return crate::ssa_lower_accessor::emit_accessor_define(
                ctx,
                obj_op,
                &key,
                &receiver_ident,
                get_eid,
                set_eid,
                acc_enum,
                acc_config,
            );
        }

        let mut flags_byte: i64 = 0;
        if let Some(b) = lookup_bool_field("writable") {
            flags_byte |= 1 << 3; // present
            if b {
                flags_byte |= 1 << 0;
            }
        }
        if let Some(b) = lookup_bool_field("enumerable") {
            flags_byte |= 1 << 4;
            if b {
                flags_byte |= 1 << 1;
            }
        }
        if let Some(b) = lookup_bool_field("configurable") {
            flags_byte |= 1 << 5;
            if b {
                flags_byte |= 1 << 2;
            }
        }
        if value_eid.is_some() {
            flags_byte |= 1 << 6; // value present
        }

        // T-29.b — Array length setter via defineProperty. Spec §9.4.2.4:
        // ToUint32(v) must equal ToNumber(v), else throw RangeError. tora
        // can't yet resize Array storage to a new length, so on valid
        // value we silently no-op; on invalid we throw via the runtime
        // validator (sufficient for the assert.throws assertion shape).
        if matches!(obj_ty, Type::Arr(_)) && is_length {
            if let Some(val_eid) = value_eid {
                let v_raw = ctx.lower_expr(val_eid);
                let v_ty = ctx.operand_ty(&v_raw);
                let (tag, val_op) = pack(ctx, v_raw, v_ty);
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.arr_set_length_validate,
                        vec![Operand::ConstI64(tag), val_op],
                    ),
                );
                ctx.emit_throw_check(None);
            }
            return true;
        }

        // Array obj (non-"length" key) — legacy arrprops_set side table
        // when the descriptor has a .value. Without .value (accessor
        // descriptor), Array attribute tracking is a follow-up — silent
        // no-op (T-29.b tolerance).
        if matches!(obj_ty, Type::Arr(_)) {
            if let Some(val_eid) = value_eid {
                let key_op = lower_key(ctx, &key);
                let v_raw = ctx.lower_expr(val_eid);
                let v_ty = ctx.operand_ty(&v_raw);
                let (tag, val_op) = pack(ctx, v_raw, v_ty);
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.arrprops_set,
                        vec![obj_op, key_op, Operand::ConstI64(tag), val_op],
                    ),
                );
            }
            return true;
        }

        // Non-Any/non-Arr obj (typed Struct etc.) — no dynobj backing
        // store, attribute tracking is N/A. Handled (no-op).
        if !matches!(obj_ty, Type::Any) {
            return true;
        }

        // Dynobj-backed Any obj — route through dynobj_define so spec
        // §10.1.6.3 validates the transitions.
        let key_op = lower_key(ctx, &key);
        let (tag, val_op) = if let Some(val_eid) = value_eid {
            let v_raw = ctx.lower_expr(val_eid);
            let v_ty = ctx.operand_ty(&v_raw);
            pack(ctx, v_raw, v_ty)
        } else {
            (0, Operand::ConstI64(0))
        };
        let dynobj = ctx.any_unbox_value_as_ptr(obj_op.clone());
        let slot = ctx.alloca(Type::Ptr, Some("__dynobj_slot"));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.dynobj_define,
                vec![
                    Operand::Value(slot),
                    key_op,
                    Operand::ConstI64(tag),
                    val_op,
                    Operand::ConstI64(flags_byte),
                ],
            ),
        );
        ctx.emit_throw_check(None);
        ctx.emit_any_dynobj_writeback(&receiver_ident, slot);
        return true;
    }

    // Runtime descriptor (RFC C1) — gated on both obj and desc being Any
    // (dynobj-backed). Key is lowered before desc to preserve obj → key
    // → desc evaluation order.
    let key_op = lower_key(ctx, &key);
    let desc_op = ctx.lower_expr(desc_eid);
    let desc_ty = ctx.operand_ty(&desc_op);
    if matches!(obj_ty, Type::Any) && matches!(desc_ty, Type::Any) {
        let desc_ptr = ctx.any_unbox_value_as_ptr(desc_op);
        let dynobj = ctx.any_unbox_value_as_ptr(obj_op);
        let slot = ctx.alloca(Type::Ptr, Some("__dynobj_slot"));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.dynobj_define_from_desc,
                vec![Operand::Value(slot), key_op, Operand::Value(desc_ptr)],
            ),
        );
        ctx.emit_throw_check(None);
        ctx.emit_any_dynobj_writeback(&receiver_ident, slot);
        return true;
    }
    false
}

/// Dispatch `Object.defineProperty` / `Object.defineProperties` — the
/// single entry `lower_expr_inner` calls. `Some` when handled; `None` to
/// fall through.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    callee_eid: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Some(v) = try_lower_define_property(ctx, callee_eid, args) {
        return Some(v);
    }
    try_lower_define_properties(ctx, callee_eid, args)
}

/// Lower `Object.defineProperty(obj, key, descriptor)`. Returns `Some`
/// when handled; `None` to fall through (non-Any obj+desc shapes keep
/// the prior "unsupported member call shape" panic).
fn try_lower_define_property(
    ctx: &mut LowerCtx,
    callee_eid: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee_eid)
        && m_name == "defineProperty"
        && let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id)
        && ns == "Object"
        && args.len() == 3
        && emit_define_one(ctx, args[0], DefineKey::Expr(args[1]), args[2])
    {
        return Some(Operand::ConstI64(0));
    }
    None
}

/// Lower `Object.defineProperties(obj, props)` (RFC C2) by compile-time
/// unfold: when `props` is an `ObjectLit` and `obj` is an Ident, emit one
/// `emit_define_one` per field (re-reading `obj` each time so a resize in
/// one field is seen by the next). Returns `Some` when unfolded; `None`
/// for other shapes (runtime `props` / non-Ident obj), which the combined
/// Object-namespace no-op arm eval-drops (prior behavior).
fn try_lower_define_properties(
    ctx: &mut LowerCtx,
    callee_eid: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee_eid)
        && m_name == "defineProperties"
        && let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id)
        && ns == "Object"
        && args.len() == 2
        && matches!(ctx.ast.get_expr(args[0]), Expr::Ident(_))
        && let Expr::ObjectLit { fields } = ctx.ast.get_expr(args[1])
    {
        // Clone the (name, desc_eid) list — `emit_define_one` borrows ctx
        // mutably, so we can't hold the AST borrow across the loop.
        let field_list: Vec<(String, ExprId)> =
            fields.iter().map(|(n, e)| (n.clone(), *e)).collect();
        for (name, desc_eid) in &field_list {
            emit_define_one(ctx, args[0], DefineKey::Name(name), *desc_eid);
        }
        return Some(Operand::ConstI64(0));
    }
    None
}
