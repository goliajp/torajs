//! `Object.defineProperty` / `Object.defineProperties` lowering —
//! carved out of `ssa_lower.rs::lower_expr_inner` so the Object
//! property-descriptor trunk (RFC
//! `.claude/rfcs/20260613-object-property-descriptors/`) grows here
//! instead of the 27k-line god-file.
//!
//! [`emit_define_one`] is the shared single-property core: a literal
//! `ObjectLit` descriptor is extracted at compile time and routed to
//! `dynobj_define` (Any obj) / `arr_set_length_validate` +
//! `arrprops_set` (Array obj) per spec §10.1.6.3 (see the [`literal`]
//! submodule); a runtime descriptor expression (RFC C1) is routed to
//! `dynobj_define_from_desc`, which reads the fields off the `desc`
//! dynobj at runtime.
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

mod literal;

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

/// RFC C4b — spec §10.1.6.3 step 1 receiver `Type(O) is Object` check
/// for `Object.defineProperty(O, ...)` / `Object.defineProperties(O, ...)`.
/// Strict Type(O) (no ToObject wrapper boxing): every primitive throws,
/// including `string` / `number` / `boolean` / `bigint` / `symbol`.
///
/// Static dispatch by the receiver's frontend type so the existing
/// typed-object / typed-Array / Any-dynobj paths downstream stay
/// reachable through their original `obj_op`:
/// * `Any` — call the runtime guard; the helper throws on primitive
///   imm / cell tags and returns silently on real object cells.
/// * Typed object — already an object, no guard needed.
/// * Anything else (`Undefined` / `Null` literal or typed primitive
///   Number / Bool / Str / Substr / Symbol / BigInt) — box the
///   receiver once and call the helper; the helper always throws on
///   these AnyValue patterns, so the post-`throw_check` normal block
///   is unreachable at runtime and `box_to_any`'s payload inc cannot
///   accumulate to a leak.
fn emit_receiver_typecheck(ctx: &mut LowerCtx, obj_eid: ExprId, obj_op: &Operand, obj_ty: Type) {
    if matches!(obj_ty, Type::Any) {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.throw_typeerror_if_not_object,
                vec![obj_op.clone()],
            ),
        );
        ctx.emit_throw_check(None);
        return;
    }
    if is_typed_object(obj_ty) {
        return;
    }
    let boxed = ctx.box_to_any_from_expr(obj_eid, obj_op.clone());
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_typeerror_if_not_object, vec![boxed]),
    );
    ctx.emit_throw_check(None);
}

/// Frontend types that are spec-`Type(O) is Object` so RFC C4b's
/// receiver guard can skip them — the SSA-level type already proves
/// the receiver is an object cell.
fn is_typed_object(ty: Type) -> bool {
    matches!(
        ty,
        Type::Obj(_)
            | Type::Arr(_)
            | Type::Closure(_)
            | Type::FnSig(_)
            | Type::RegExp
            | Type::Date
            | Type::Promise
            | Type::Map
            | Type::Set
            | Type::MapIter
            | Type::ArrIter
            | Type::WeakRef
            | Type::WeakMap
            | Type::WeakSet
    )
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

    emit_receiver_typecheck(ctx, obj_eid, &obj_op, obj_ty);

    let is_length = key_is_length(ctx, &key);

    // Compile-time literal descriptor — extract value + the three data
    // flags from the ObjectLit at compile time.
    if matches!(ctx.ast.get_expr(desc_eid), Expr::ObjectLit { .. }) {
        return literal::emit_define_literal(
            ctx,
            obj_op,
            obj_ty,
            &key,
            &receiver_ident,
            desc_eid,
            is_length,
        );
    }
    emit_define_runtime_desc(ctx, obj_op, obj_ty, &key, &receiver_ident, desc_eid)
}

/// Runtime descriptor (RFC C1) — gated on both obj and desc being Any
/// (dynobj-backed). Key is lowered before desc to preserve obj → key
/// → desc evaluation order.
fn emit_define_runtime_desc(
    ctx: &mut LowerCtx,
    obj_op: Operand,
    obj_ty: Type,
    key: &DefineKey,
    receiver_ident: &Option<String>,
    desc_eid: ExprId,
) -> bool {
    let key_op = lower_key(ctx, key);
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
        ctx.emit_any_dynobj_writeback(receiver_ident, slot);
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
        && args.len() >= 3
        && emit_define_one(ctx, args[0], DefineKey::Expr(args[1]), args[2])
    {
        // S317 — ES §20.1.2.6 silently ignores args past (obj, key,
        // desc). `emit_define_one` lowers args[0..3] (obj + key +
        // desc fields); lower-and-drop args[3..] after for spec
        // left-to-right side-effect order.
        for &a in args.iter().skip(3) {
            let _ = ctx.lower_expr(a);
        }
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
    {
        // RFC C4b — spec §20.1.2.4 step 1 receiver guard. Fires for every
        // shape (incl. non-Ident / non-ObjectLit) so a primitive receiver
        // throws even when the prior fall-through to the Object no-op arm
        // would have silently eval-dropped the args.
        let obj_raw = ctx.lower_expr(args[0]);
        let obj_ty = ctx.operand_ty(&obj_raw);
        emit_receiver_typecheck(ctx, args[0], &obj_raw, obj_ty);

        // Compile-time unfold when both shapes match — Ident lower is
        // idempotent, so emit_define_one's per-field re-lower is safe.
        if matches!(ctx.ast.get_expr(args[0]), Expr::Ident(_))
            && let Expr::ObjectLit { fields } = ctx.ast.get_expr(args[1])
        {
            // Clone the (name, desc_eid) list — `emit_define_one` borrows ctx
            // mutably, so we can't hold the AST borrow across the loop.
            let field_list: Vec<(String, ExprId)> =
                fields.iter().map(|(n, e)| (n.clone(), *e)).collect();
            for (name, desc_eid) in &field_list {
                emit_define_one(ctx, args[0], DefineKey::Name(name), *desc_eid);
            }
        } else {
            // Runtime props / non-Ident receiver — eval-drop the props
            // arg for side effects; full runtime walk is a follow-up.
            let _ = ctx.lower_expr(args[1]);
        }
        // RFC 20260705 owned-result invariant: ES answers the receiver;
        // the pass-through result carries its own ref (this dedicated
        // path bypasses integrity's lower_noop — the 545 gate caught
        // the blanket discard over-releasing the un-inc'd receiver).
        ctx.emit_owned_result_inc(obj_raw.clone(), obj_ty);
        return Some(obj_raw);
    }
    None
}
