//! `Object.create(proto[, descriptors])` lowering — §20.1.2.2.
//! Split out of `ssa_lower_call_object_integrity.rs` (file-size hard
//! limit; the RFC 20260717-user-proto-chain knife-1 proto-link wiring
//! pushed the parent past 500). Dispatch stays in the parent's
//! `try_lower`; this sibling owns the create arm only.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
/// §20.1.2.2 steps 1-2 — validate the proto argument and keep its
/// Any box for the OrdinaryObjectCreate wire below.
fn resolve_proto_link(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<(ExprId, Operand, Operand, bool)> {
    let mut proto_link: Option<(ExprId, Operand, Operand, bool)> = None;
    if let Some(&proto_eid) = args.first() {
        let proto_op = ctx.lower_expr(proto_eid);
        let proto_ty = ctx.operand_ty(&proto_op);
        let proto_owned = (ctx.expr_owned_shape(proto_eid)
            || ctx.expr_minted_closure(proto_eid, &proto_op))
            && !proto_ty.is_copy();
        // Statically-proven object cells skip the runtime check; Any
        // and primitive shapes route through the helper (null passes,
        // every other primitive throws — for a typed primitive the
        // post-throw-check block is unreachable, so box_to_any's
        // payload inc cannot accumulate).
        if !crate::ssa_lower_object_define::is_typed_object(proto_ty.clone()) {
            let boxed = if matches!(proto_ty, Type::Any) {
                proto_op.clone()
            } else {
                ctx.box_to_any_from_expr(proto_eid, proto_op.clone())
            };
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.object_create_check_proto,
                    vec![boxed.clone()],
                ),
            );
            if proto_owned {
                ctx.emit_throw_check_owned(None, proto_op.clone(), proto_ty);
            } else {
                ctx.emit_throw_check(None);
            }
            proto_link = Some((proto_eid, proto_op, boxed, proto_owned));
        } else {
            let boxed = ctx.box_to_any_from_expr(proto_eid, proto_op.clone());
            proto_link = Some((proto_eid, proto_op, boxed, proto_owned));
        }
    }
    proto_link
}

/// §20.1.2.2 step 3 — evaluate the props argument. `None` skips
/// ObjectDefineProperties entirely.
///
/// An owned proto temp is still live throughout (its link — and
/// release — happen only after the alloc below); the caller parks it
/// on `temps.throw_live` before calling, so every throw path in here
/// drops it without per-site plumbing (rotation 549 — this replaced
/// the hand-carried `proto_link` owned-drop on the static-null path).
fn resolve_props(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<(ExprId, Operand, bool)> {
    // §20.1.2.2 step 3 — evaluate the props arg. A literal ObjectLit
    // forces the dynobj lane (the desc-of-descs tree must be
    // dynobj-backed for the runtime walk; the fresh tree is an owned
    // Any temp released after the walk); an Any variable is a borrow.
    //
    // 2026-07-16 (rotation 121 chunk 2) — a statically-null props arg
    // (`Object.create(x, null)`) must throw TypeError per
    // §20.1.2.2 step 3 → `ObjectDefineProperties` step 1 → `ToObject`
    // (`null` fails the ToObject conversion; `undefined` skips step 3
    // and is legal — hence a static-null-only gate, not a nullish
    // one). The existing `throw_typeerror_if_props_nullish` helper
    // throws on both null and undefined AnyValues, which is exactly
    // what we want at this static-null site: encode a compile-time
    // ANY_NULL box and hand it through. Dyn-null (an `any` variable
    // holding null) stays a residual — L3b entry for the general
    // runtime nullish gate (would need a spec-Object.create-shape
    // helper that only throws on null, since dyn-undefined must
    // fall through to skip step 3).
    match args.get(1) {
        Some(&p_eid)
            if matches!(ctx.ast.get_expr(p_eid), Expr::Null)
                || matches!(ctx.expr_types.get(&p_eid), Some(crate::check::Type::Null)) =>
        {
            let _ = ctx.lower_expr(p_eid);
            let null_any = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(0 /* ANY_NULL */), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.throw_typeerror_if_props_nullish,
                    vec![Operand::Value(null_any)],
                ),
            );
            // An owned proto temp still live on this always-throwing
            // path is parked on `temps.throw_live` by the caller —
            // the plain check drops it in the throw block.
            ctx.emit_throw_check(None);
            None
        }
        Some(&p_eid) if matches!(ctx.ast.get_expr(p_eid), Expr::ObjectLit { .. }) => {
            let d = ctx.lower_dynobj_init(p_eid);
            let boxed = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(4 /* ANY_HEAP */), d],
                ),
                Type::Any,
                None,
            );
            Some((p_eid, Operand::Value(boxed), true))
        }
        Some(&p_eid) => Some((p_eid, ctx.lower_expr(p_eid), false)),
        None => None,
    }
}

/// [`emit_props_walk`] with the fresh dynobj's throw-path parking
/// carried across it (rotation 549): the walk can relocate the block,
/// so the pre-walk parked pointer is unparked first and the post-walk
/// reload is parked in its place before the caller's throw check.
fn walk_reparked(
    ctx: &mut LowerCtx<'_>,
    dynobj: crate::ssa::ValueId,
    dynobj_tok: usize,
    props_ptr: crate::ssa::ValueId,
) -> (crate::ssa::ValueId, usize) {
    ctx.pop_throw_temp(dynobj_tok);
    let dynobj = emit_props_walk(ctx, dynobj, props_ptr);
    let reparked = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::PtrToInt(Operand::Value(dynobj)),
        Type::Any,
        None,
    );
    let tok = ctx.push_throw_temp(Operand::Value(reparked), Type::Any);
    (dynobj, tok)
}

/// §20.1.2.2 step 3 — ObjectDefineProperties(obj, props) via the
/// two-phase runtime walk. Defines can resize the dense array, so the
/// dynobj rides a slot and the post-walk pointer reloads from it;
/// returns the reloaded pointer. Shared by the Any lane (gated cell)
/// and the typed-struct lane (the struct operand is the cell).
fn emit_props_walk(
    ctx: &mut LowerCtx<'_>,
    dynobj: crate::ssa::ValueId,
    props_ptr: crate::ssa::ValueId,
) -> crate::ssa::ValueId {
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
    let cur_block = ctx.cur_block;
    ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::Ptr, Operand::Value(slot), 0),
        Type::Ptr,
        None,
    )
}

/// `Object.create(proto[, descriptors])` — §20.1.2.2. Validates the
/// proto argument (Object or null, else TypeError) then allocates a
/// fresh dynobj-backed Any-box. The proto value itself is not yet
/// wired to the new object (no per-object proto slot — RFC
/// 20260712-object-create-define-props backlog); the descriptors arg
/// is evaluated for side effects (props processing is RFC chunk 2).
pub(crate) fn lower_create(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    // §20.1.2.2 steps 1-2 — validate the proto and keep its Any box
    // alive across the props evaluation so the fresh dynobj can link
    // it (RFC 20260717-user-proto-chain knife 1). The raw operand is
    // released only AFTER the link call below; throw paths in between
    // carry the owned temp through `emit_throw_check_owned` so the
    // deferred release cannot strand a reference.
    let mut proto_link = resolve_proto_link(ctx, args);
    // Rotation 549 — the owned proto temp lives until the link call
    // far below; park it so every throw path in between (props
    // gates, props walk, extra-arg lowers) drops it.
    let proto_tok = if let Some((_, ref p_op, _, true)) = proto_link {
        let p_ty = ctx.operand_ty(p_op);
        let p = p_op.clone();
        Some(ctx.push_throw_temp(p, p_ty))
    } else {
        None
    };
    let props = resolve_props(ctx, args);
    // An owned props temp (literal desc-of-descs tree, fresh call
    // product) lives across the gates and the walk — park it too.
    let props_tok = props.as_ref().and_then(|(p_eid, p_op, is_literal)| {
        if *is_literal {
            let p = p_op.clone();
            Some(ctx.push_throw_temp(p, Type::Any))
        } else {
            ctx.throw_temp_of(*p_eid, p_op)
                .map(|(op, ty)| ctx.push_throw_temp(op, ty))
        }
    });
    for &a in args.iter().skip(2) {
        let op = ctx.lower_expr(a);
        ctx.release_owned_temp(a, &op);
    }
    let cur_block = ctx.cur_block;
    let mut dynobj = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.dynobj_alloc, vec![]),
        Type::Ptr,
        None,
    );
    // The fresh dynobj is owned by nobody until the final Any box —
    // park it (as its Any bit pattern; a cell encodes as its pointer)
    // so a props-walk throw drops it instead of stranding it. The
    // walk can relocate the block, so the parked value is re-parked
    // from the post-walk reload before the walk's throw check runs.
    let dynobj_any = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::PtrToInt(Operand::Value(dynobj)),
        Type::Any,
        None,
    );
    let mut dynobj_tok = ctx.push_throw_temp(Operand::Value(dynobj_any), Type::Any);
    // §20.1.2.2 step 2 — OrdinaryObjectCreate(proto): wire the
    // validated proto onto the fresh dict (a heap cell lands in the
    // own `__proto__` simulation slot with its own +1; null — static
    // or runtime-Any, closing the recorded residual — sets the
    // null-prototype header bit). Runs before the props walk (which
    // may relocate the block) so the alloc-fresh pointer is valid,
    // and cannot grow the dict (first entry, initial cap).
    if let Some((proto_eid, proto_op, boxed, _)) = proto_link.take() {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.object_create_link_proto,
                vec![Operand::Value(dynobj), boxed],
            ),
        );
        // Unpark before the unconditional release — later throw
        // checks (props walk) must not drop the proto again.
        if let Some(t) = proto_tok {
            ctx.pop_throw_temp(t);
        }
        ctx.release_owned_temp(proto_eid, &proto_op);
    }
    // §20.1.2.2 step 3 — ObjectDefineProperties(obj, props) on the
    // fresh dynobj via the two-phase runtime walk; defines can resize
    // the dense array, so the post-walk pointer reloads from the slot.
    if let Some((p_eid, p_op, is_literal)) = props {
        let p_ty = ctx.operand_ty(&p_op);
        if matches!(
            p_ty,
            Type::Obj(_) | Type::Closure(_) | Type::FnSig(_) | Type::Arr(_)
        ) {
            // An As-cast / typed heap props (`Object.create(null,
            // p)` with a desc-of-descs binding, or a Closure / Arr
            // carrying descriptor expandos — RFC 20260721 刀 2) is an
            // SSA pass-through — the Any arm below never fires and
            // the walk was skipped (silent no-define). The typed
            // operand IS the cell ptr; the kernel's walkable-source
            // dispatch owns the per-tag walk (mirror of
            // defineProperties' widened lane).
            let props_ptr = match p_op.clone() {
                Operand::Value(v) => v,
                _ => ctx.any_unbox_value_as_ptr(p_op.clone()),
            };
            (dynobj, dynobj_tok) = walk_reparked(ctx, dynobj, dynobj_tok, props_ptr);
            if let Some(t) = props_tok {
                ctx.pop_throw_temp(t);
            }
            ctx.release_owned_temp(p_eid, &p_op);
            ctx.emit_throw_check(None);
        } else if matches!(p_ty, Type::Any) {
            // §20.1.2.2 step 3 — `null` fails ToObject; `undefined`
            // skips the whole clause. The static-null literal is
            // already stripped above; a dyn `Any` variable (e.g.
            // `const p: any = null; Object.create({}, p)`) reaches
            // this arm and must throw before the walk sees a
            // null-immediate that unboxes to a NULL pointer (the
            // runtime helper then silent-returns → bug). The gate
            // is spec-correct on `undefined` too (helper is
            // null-only, not nullish — undefined must fall through
            // to skip step 3).
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.throw_typeerror_if_props_null_only,
                    vec![p_op.clone()],
                ),
            );
            ctx.emit_throw_check(None);
            // §20.1.2.2 step 3 → ObjectDefineProperties step 1
            // ToObject: the gate answers the walkable cell, or NULL
            // for the primitive no-op faces (the kernel no-ops on
            // NULL); the non-empty-string face records its TypeError
            // inside. Pre-fix the raw unbox handed immediate bits to
            // the kernel as a pointer (`Object.create(null, 1)`
            // SIGSEGV'd).
            let props_ptr = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.define_props_source_gate, vec![p_op.clone()]),
                Type::Ptr,
                None,
            );
            ctx.emit_throw_check(None);
            (dynobj, dynobj_tok) = walk_reparked(ctx, dynobj, dynobj_tok, props_ptr);
            if let Some(t) = props_tok {
                ctx.pop_throw_temp(t);
            }
            if is_literal {
                ctx.emit_drop_value(p_op, Type::Any);
            } else {
                ctx.release_owned_temp(p_eid, &p_op);
            }
            ctx.emit_throw_check(None);
        } else {
            // A typed non-object props (`"ab" as any` — the As cast is
            // an SSA pass-through, so the operand keeps Type::Str and
            // the Any arm above never fires) must still hit the
            // §20.1.2.3.1 step 1 ToObject faces: the non-empty-string
            // face throws inside the gate; every other primitive
            // answers NULL, so no walk is emitted. Static null is
            // already stripped in `resolve_props`, and a typed
            // `undefined` boxes to ANY_UNDEF which the gate no-ops —
            // both spec-correct without a nullish pre-gate.
            if !crate::ssa_lower_object_define::is_typed_object(p_ty.clone()) {
                let boxed = ctx.box_to_any_from_expr(p_eid, p_op.clone());
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.define_props_source_gate, vec![boxed]),
                    Type::Ptr,
                    None,
                );
                ctx.emit_throw_check(None);
            }
            if let Some(t) = props_tok {
                ctx.pop_throw_temp(t);
            }
            ctx.release_owned_temp(p_eid, &p_op);
        }
    }
    // The Any box below is the dynobj's owner from here on.
    ctx.pop_throw_temp(dynobj_tok);
    let cur_block = ctx.cur_block;
    let box_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(4 /* ANY_HEAP */), Operand::Value(dynobj)],
        ),
        Type::Any,
        None,
    );
    Operand::Value(box_v)
}
