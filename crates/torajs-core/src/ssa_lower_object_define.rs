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
pub(crate) fn is_typed_object(ty: Type) -> bool {
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

/// Lower the key to a `Str` operand. Call at the spec evaluation point
/// (after `obj`, before the descriptor) so a side-effecting key expr
/// orders correctly.
///
/// Returns `(key_op, owned)` — `owned == true` means the caller must
/// emit a `str_drop` on `key_op` after the runtime helper borrows it
/// (RFC 20260716 刀 18 — ToPropertyKey coerce). `false` covers the
/// interned-literal `DefineKey::Name` shape and the `Type::Str` Expr
/// fast path (both are borrow-shaped shares of a stable Str).
pub(crate) fn lower_key(ctx: &mut LowerCtx, key: &DefineKey) -> (Operand, bool) {
    match key {
        DefineKey::Expr(eid) => {
            let raw = ctx.lower_expr(*eid);
            let ty = ctx.operand_ty(&raw);
            match ty {
                Type::Str => (raw, false),
                // RFC 20260716 刀 18 — ES §20.1.2.6 step 1 / §20.1.2.10
                // step 1 → §7.1.19 ToPropertyKey → §7.1.17 ToString.
                // StringWrapper / Number / Boolean / etc. keys route
                // through `emit_to_string`; returned Str is owned and
                // callers drop after the helper borrow read.
                _ => {
                    let coerced =
                        crate::ssa_lower_call_coercion::emit_to_string(ctx, *eid, raw, ty);
                    (coerced, true)
                }
            }
        }
        DefineKey::Name(n) => (Operand::Value(ctx.intern_string_literal(n)), false),
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

    emit_receiver_typecheck(ctx, obj_eid, &obj_op, obj_ty.clone());
    emit_define_one_core(ctx, obj_op, obj_ty, &receiver_ident, key, desc_eid)
}

/// [`emit_define_one`] with the receiver already lowered and
/// typechecked — `defineProperties`' non-Ident-receiver unfold lowers
/// the receiver once and feeds every field through here (a per-field
/// re-lower of an ObjectLit receiver would mint a fresh object each
/// time).
fn emit_define_one_core(
    ctx: &mut LowerCtx,
    obj_op: Operand,
    obj_ty: Type,
    receiver_ident: &Option<String>,
    key: DefineKey,
    desc_eid: ExprId,
) -> bool {
    // Compile-time literal descriptor — extract value + the three data
    // flags from the ObjectLit at compile time. The fast path declines
    // (false) when a flag field is not a Bool literal; the descriptor
    // then materializes as a dynobj and takes the full runtime
    // ToPropertyDescriptor path (§6.2.6.5 ToBoolean semantics).
    if matches!(ctx.ast.get_expr(desc_eid), Expr::ObjectLit { .. }) {
        if literal::emit_define_literal(
            ctx,
            obj_op.clone(),
            obj_ty.clone(),
            &key,
            receiver_ident,
            desc_eid,
        ) {
            return true;
        }
        return emit_define_objlit_runtime(ctx, obj_op, obj_ty, &key, receiver_ident, desc_eid);
    }
    emit_define_runtime_desc(ctx, obj_op, obj_ty, &key, receiver_ident, desc_eid)
}

/// Literal descriptor the fast path declined — materialize the
/// ObjectLit as a fresh dynobj (owned temp, released after the call)
/// and route through `__torajs_dynobj_define_from_desc`, whose
/// `define_apply` entry carries the Arr receiver dispatch.
fn emit_define_objlit_runtime(
    ctx: &mut LowerCtx,
    obj_op: Operand,
    obj_ty: Type,
    key: &DefineKey,
    receiver_ident: &Option<String>,
    desc_eid: ExprId,
) -> bool {
    let obj_ptr: Operand = match &obj_ty {
        Type::Any => Operand::Value(ctx.any_unbox_value_as_ptr(obj_op)),
        Type::Arr(_) => {
            // Kernel element writes are kind-aware — mark at the
            // reflection boundary (mirror of the literal Arr arm).
            ctx.emit_arr_mark_kind(&obj_op);
            obj_op
        }
        // Typed Struct etc. — no dynobj backing store, attribute
        // tracking N/A (same handled-no-op contract as the literal
        // arm).
        _ => return true,
    };
    let (key_op, key_owned) = lower_key(ctx, key);
    let desc_ptr = ctx.lower_dynobj_init(desc_eid);
    let slot = ctx.alloca(Type::Ptr, Some("__dynobj_slot"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(obj_ptr, Operand::Value(slot), 0),
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.dynobj_define_from_desc,
            vec![Operand::Value(slot), key_op.clone(), desc_ptr.clone()],
        ),
    );
    // Release the fresh descriptor before the throw-check (mirror of
    // Object.create's literal-props drop).
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.value_drop_heap, vec![desc_ptr]),
    );
    // 刀 18 — coerced key was owned Str; drop after helper borrowed it.
    if key_owned {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_drop, vec![key_op]),
        );
    }
    ctx.emit_throw_check(None);
    if matches!(obj_ty, Type::Any) {
        ctx.emit_any_dynobj_writeback(receiver_ident, slot);
    }
    true
}

/// Runtime descriptor (RFC C1) — obj is Any (dynobj-backed) or a
/// typed Array (define_apply's TAG_ARR dispatch); desc is Any or a
/// typed Closure heap cell (its shape resolves at the
/// `define_from_desc` entry — a Closure descriptor reads through its
/// expando props dynobj). Key is lowered before desc to preserve
/// obj → key → desc evaluation order.
fn emit_define_runtime_desc(
    ctx: &mut LowerCtx,
    obj_op: Operand,
    obj_ty: Type,
    key: &DefineKey,
    receiver_ident: &Option<String>,
    desc_eid: ExprId,
) -> bool {
    let (key_op, key_owned) = lower_key(ctx, key);
    let desc_op = ctx.lower_expr(desc_eid);
    let desc_ty = ctx.operand_ty(&desc_op);
    let desc_ptr: Operand = match desc_ty {
        // §6.2.6.5 step 1 gate BEFORE the unbox — an imm AnyValue's
        // payload is not a cell (the old path handed a number's bits
        // to define_from_desc as a pointer), and null / undefined /
        // primitive cells must throw (RFC 20260713 chunk B).
        Type::Any => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.throw_typeerror_if_not_desc_object,
                    vec![desc_op.clone()],
                ),
            );
            ctx.emit_throw_check(None);
            Operand::Value(ctx.any_unbox_value_as_ptr(desc_op))
        }
        // Heap cells whose shape the define_from_desc entry resolves
        // (Closure / Arr expando props dynobj at +24). Typed structs
        // stay declined — no dynobj-backed own domain (recorded
        // divergence, same reflection boundary as prop_delete's
        // Tag::Obj arm).
        Type::Closure(_) | Type::Arr(_) => desc_op,
        _ if is_typed_object(desc_ty) => return false,
        // Statically-known primitive descriptor (`defineProperty(o,
        // "k", 5)` / `null` literal) — box once and let the helper
        // throw the §6.2.6.5 step 1 TypeError (same shape as
        // `emit_receiver_typecheck`'s primitive arm: the post-check
        // block is unreachable at runtime). Decided before the
        // obj-storage gate below: the throw needs no receiver storage,
        // so `defineProperty({}, "k", 5)` / an unfolded `{a: null}`
        // field throws instead of falling through.
        _ => {
            let boxed = ctx.box_to_any_from_expr(desc_eid, desc_op.clone());
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.throw_typeerror_if_not_desc_object,
                    vec![boxed],
                ),
            );
            ctx.emit_throw_check(None);
            return true;
        }
    };
    // Receiver storage gate — only Any (dynobj-backed) and typed Arr
    // receivers carry a define surface here; other shapes keep the
    // caller's fall-through.
    if !matches!(obj_ty, Type::Any | Type::Arr(_)) {
        return false;
    }
    let obj_ptr: Operand = match &obj_ty {
        Type::Any => Operand::Value(ctx.any_unbox_value_as_ptr(obj_op)),
        _ => {
            // Kernel element writes are kind-aware — mark at the
            // reflection boundary (mirror of the literal Arr arm).
            ctx.emit_arr_mark_kind(&obj_op);
            obj_op
        }
    };
    let slot = ctx.alloca(Type::Ptr, Some("__dynobj_slot"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(obj_ptr, Operand::Value(slot), 0),
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.dynobj_define_from_desc,
            vec![Operand::Value(slot), key_op.clone(), desc_ptr],
        ),
    );
    // 刀 18 — coerced key was owned Str; drop after helper borrowed it.
    if key_owned {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_drop, vec![key_op]),
        );
    }
    ctx.emit_throw_check(None);
    if matches!(obj_ty, Type::Any) {
        ctx.emit_any_dynobj_writeback(receiver_ident, slot);
    }
    true
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

        // Compile-time unfold on an ObjectLit props. An Ident receiver
        // re-lowers per field (idempotent, and a resize in one field is
        // seen by the next); any other receiver shape reuses the
        // already-lowered operand — a per-field re-lower of an
        // ObjectLit receiver would mint a fresh object each time (RFC
        // 20260713 chunk B: `defineProperties({}, {a: null})` must
        // reach the per-field §6.2.6.5 TypeError).
        if let Expr::ObjectLit { fields } = ctx.ast.get_expr(args[1]) {
            // Clone the (name, desc_eid) list — `emit_define_one` borrows ctx
            // mutably, so we can't hold the AST borrow across the loop.
            let field_list: Vec<(String, ExprId)> =
                fields.iter().map(|(n, e)| (n.clone(), *e)).collect();
            let obj_is_ident = matches!(ctx.ast.get_expr(args[0]), Expr::Ident(_));
            for (name, desc_eid) in &field_list {
                if obj_is_ident {
                    emit_define_one(ctx, args[0], DefineKey::Name(name), *desc_eid);
                } else {
                    emit_define_one_core(
                        ctx,
                        obj_raw.clone(),
                        obj_ty.clone(),
                        &None,
                        DefineKey::Name(name),
                        *desc_eid,
                    );
                }
            }
        } else {
            // RFC 20260712 chunk 2 — runtime props walk: both shapes
            // Any (dynobj-backed) route through the two-phase
            // §20.1.2.3.1 helper; anything else keeps the prior
            // eval-drop (typed receivers are the RFC backlog).
            let receiver_ident: Option<String> = if let Expr::Ident(n) = ctx.ast.get_expr(args[0]) {
                Some(n.clone())
            } else {
                None
            };
            let props_op = ctx.lower_expr(args[1]);
            let props_ty = ctx.operand_ty(&props_op);
            // §20.1.2.3.1 step 2 — ToObject(Properties) throws only
            // on null / undefined (other primitives wrap to key-less
            // objects; the walk no-ops). Statically-typed non-object
            // props box once so the helper can throw; a runtime Any
            // gates before the walk (RFC 20260713 chunk B).
            if !matches!(props_ty, Type::Any) && !is_typed_object(props_ty.clone()) {
                let boxed = ctx.box_to_any_from_expr(args[1], props_op.clone());
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.throw_typeerror_if_props_nullish, vec![boxed]),
                );
                ctx.emit_throw_check(None);
            } else if matches!(props_ty, Type::Any) {
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.throw_typeerror_if_props_nullish,
                        vec![props_op.clone()],
                    ),
                );
                ctx.emit_throw_check(None);
            }
            if matches!(obj_ty, Type::Any) && matches!(props_ty, Type::Any) {
                let props_ptr = ctx.any_unbox_value_as_ptr(props_op.clone());
                let dynobj = ctx.any_unbox_value_as_ptr(obj_raw.clone());
                let slot = ctx.alloca(Type::Ptr, Some("__dynobj_slot"));
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
                );
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.dynobj_define_properties_from,
                        vec![Operand::Value(slot), Operand::Value(props_ptr)],
                    ),
                );
                ctx.release_owned_temp(args[1], &props_op);
                ctx.emit_throw_check(None);
                ctx.emit_any_dynobj_writeback(&receiver_ident, slot);
            } else {
                ctx.release_owned_temp(args[1], &props_op);
            }
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
