//! Object.* integrity / meta trap cluster (`Object.freeze` /
//! `Object.isFrozen` / `Object.create` / `Object.preventExtensions` /
//! `Object.isExtensible` / `Object.seal` / `Object.isSealed` /
//! no-op pass-through for `Object.setPrototypeOf` /
//! `Object.defineProperties` / `Object.assign`) pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-28 of the `Expr::Call` god-arm decomp (chunks 1-27 = ... +
//! Promise.<static>).
//!
//! Eight arms share the `Object.<m>(obj, ...)` namespace shape:
//!
//! - **T-09.d (v0.4.0)** — `Object.freeze(obj)` sets `FLAG_FROZEN` and
//!   returns the same obj; `Object.isFrozen(obj)` reads the bit.
//!   Primitive guard (Bool / I64 / F64): freeze returns the value
//!   unchanged; isFrozen returns `true` per ES2015 §15.2.3.9 / 12 —
//!   the runtime helpers would otherwise deref a non-heap bit
//!   pattern as a heap header (SIGSEGV). Any-typed receivers route
//!   through `obj_is_frozen_any` which is NaN-box aware.
//! - **2026-05-18** — `Object.create(proto[, descriptors])` allocates
//!   a fresh dynobj-backed Any-box. Args evaluated for side effects
//!   then discarded (descriptors arg ignored in the subset; most
//!   test262 usages are `Object.create(null)` for table-style
//!   storage).
//! - **RFC C5b** — `Object.preventExtensions(obj)` flips
//!   `FLAG_NON_EXTENSIBLE` on the heap header (cell case) or returns
//!   the primitive as-is per §20.1.2.16 step 1. Boxes typed
//!   receivers to Any so the helper's single ABI handles every
//!   receiver shape; cell still points at the same heap block so
//!   `Object.preventExtensions(o) === o` holds.
//! - **RFC C5b** — `Object.isExtensible(obj)` reads
//!   `FLAG_NON_EXTENSIBLE` (cell case) or returns `false` per
//!   §20.1.2.13 step 1.
//! - **RFC C5b-3** — `Object.seal(obj)` sets both
//!   `FLAG_NON_EXTENSIBLE` + `FLAG_SEALED` and walks the DynObj
//!   entry table to clear `configurable` on each property. Non-
//!   DynObj typed cells carry only the header markers (their
//!   property table is the static field layout).
//! - **RFC C5b-3** — `Object.isSealed(obj)` is `true` iff
//!   `[[Extensible]] = false` AND every own property is non-
//!   configurable. Spec primitives are vacuously sealed (`true`);
//!   cells dispatch through the runtime helper.
//! - **2026-05-18 no-op cluster** — `Object.setPrototypeOf` /
//!   `Object.defineProperties` / `Object.assign` are subset no-ops
//!   here (tora has no real prototype chain for dynobj-backed
//!   objects yet; the chunk-20 `ssa_lower_call_object_assign` arm
//!   runs first and claims the structural-copy `Object.assign`
//!   path — this arm is the fallback that returns `obj` per spec
//!   when that sibling falls through). Extra args evaluated for
//!   side effects then discarded.
//!
//! Returns `Some(op)` on hit; `None` on miss so the caller falls
//! through to subsequent arms or the generic call lowering.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    let recv_id = *ns_id;
    let method = m_name.clone();
    let Expr::Ident(ns) = ctx.ast.get_expr(recv_id) else {
        return None;
    };
    if args.is_empty() {
        return None;
    }
    // §28.1.{8,10} Reflect.isExtensible / preventExtensions
    // (rotation 266 刀 R2) — the Object-flavor kernels with the
    // strict IsObject gate in front (every primitive throws, no
    // primitive pass-through). preventExtensions answers boolean
    // true (ordinary [[PreventExtensions]] always succeeds), not
    // the receiver.
    if ns == "Reflect" {
        return match method.as_str() {
            "preventExtensions" => Some(
                crate::ssa_lower_call_reflect_integrity::lower_reflect_extensible(ctx, args, true),
            ),
            "isExtensible" => Some(
                crate::ssa_lower_call_reflect_integrity::lower_reflect_extensible(ctx, args, false),
            ),
            "deleteProperty" if args.len() >= 2 => Some(
                crate::ssa_lower_call_reflect_integrity::lower_reflect_delete_property(ctx, args),
            ),
            "defineProperty" if args.len() >= 3 => Some(
                crate::ssa_lower_call_reflect_integrity::lower_reflect_define_property(ctx, args),
            ),
            "apply" if args.len() >= 3 => {
                Some(crate::ssa_lower_call_reflect_integrity::lower_reflect_apply(ctx, args))
            }
            "construct" if args.len() >= 2 => {
                Some(crate::ssa_lower_call_reflect_integrity::lower_reflect_construct(ctx, args))
            }
            "setPrototypeOf" if args.len() >= 2 => Some(
                crate::ssa_lower_call_reflect_integrity::lower_reflect_set_prototype_of(ctx, args),
            ),
            "set" if args.len() >= 3 => Some(crate::ssa_lower_call_reflect_set::lower_reflect_set(
                ctx, args,
            )),
            _ => None,
        };
    }
    if ns != "Object" {
        return None;
    }
    match method.as_str() {
        "freeze" | "isFrozen" => Some(lower_freeze_or_is_frozen(ctx, method.as_str(), args)),
        "create" => Some(crate::ssa_lower_call_object_create::lower_create(ctx, args)),
        "preventExtensions" => Some(lower_prevent_extensions(ctx, args)),
        "isExtensible" => Some(lower_is_extensible(ctx, args)),
        "seal" => Some(lower_seal(ctx, args)),
        "isSealed" => Some(lower_is_sealed(ctx, args)),
        "setPrototypeOf" => Some(lower_set_prototype_of(ctx, args)),
        "defineProperties" | "assign" => Some(lower_noop(ctx, args)),
        _ => None,
    }
}

/// `Object.freeze(obj)` / `Object.isFrozen(obj)` — primitive short
/// circuit + full integrity-level walk (freeze) or heap-header bit
/// read (isFrozen).
///
/// RFC 20260716 刀 24 — `freeze` routes through the box-aware
/// `anyv_freeze` for every cell shape (mirrors the `anyv_seal` shape)
/// so `getOwnPropertyDescriptor` / `isSealed` / `isExtensible`
/// observe the frozen level correctly (spec: frozen ⇒ sealed ⇒
/// non-extensible + every own property writable/configurable false).
/// `isFrozen` keeps the header-only fast path.
fn lower_freeze_or_is_frozen(ctx: &mut LowerCtx<'_>, method: &str, args: &[ExprId]) -> Operand {
    let arg_op = ctx.lower_expr(args[0]);
    let arg_ty = ctx.operand_ty(&arg_op);
    let is_primitive = matches!(arg_ty, Type::I64 | Type::F64 | Type::Bool);
    if is_primitive {
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        return if method == "freeze" {
            arg_op
        } else {
            Operand::ConstBool(true)
        };
    }
    if method == "freeze" {
        // Typed FnSig (closure) has no dynobj entry table to walk +
        // `box_to_any_from_expr` doesn't support FnSig yet, so route
        // it through the header-only `obj_freeze` (which was updated
        // to set FLAG_FROZEN + FLAG_SEALED + FLAG_NON_EXTENSIBLE
        // together — spec: frozen ⇒ sealed ⇒ non-extensible).
        // Every other non-primitive shape can box into an Any and
        // route through `anyv_freeze` for the per-entry walk.
        if matches!(arg_ty, Type::FnSig(_)) {
            for &a in args.iter().skip(1) {
                let _ = ctx.lower_expr(a);
            }
            let cur_block = ctx.cur_block;
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.obj_freeze, vec![arg_op]),
                arg_ty,
                None,
            );
            // RFC 20260705 owned-result invariant: `obj_freeze` passes
            // the receiver through un-inc'd; the result carries its
            // own ref.
            ctx.emit_owned_result_inc(Operand::Value(v), arg_ty);
            return Operand::Value(v);
        }
        // Any-box first (parity with `Object.seal` — the anyv helper
        // owns the NaN-box tag dispatch + primitive short-circuit that
        // a raw pointer wouldn't handle).
        let any_op = if matches!(arg_ty, Type::Any) {
            arg_op
        } else {
            ctx.box_to_any_from_expr(args[0], arg_op)
        };
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.anyv_freeze, vec![any_op]),
            Type::Any,
            None,
        );
        // RFC 20260705 owned-result invariant: anyv_freeze answers the
        // receiver box un-inc'd; the result carries its own ref.
        ctx.emit_owned_result_inc(Operand::Value(v), Type::Any);
        return Operand::Value(v);
    }
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let (fid, ret_ty) = if arg_ty == Type::Any {
        (ctx.intrinsics.obj_is_frozen_any, Type::Bool)
    } else {
        (ctx.intrinsics.obj_is_frozen, Type::Bool)
    };
    let cur_block = ctx.cur_block;
    let v = ctx
        .f
        .append_inst(cur_block, InstKind::Call(fid, vec![arg_op]), ret_ty, None);
    Operand::Value(v)
}

/// `Object.preventExtensions(obj)` — flip `FLAG_NON_EXTENSIBLE`. Boxes
/// typed receivers to Any so the helper has a single ABI.
///
/// ObjectLit-literal-arg 强 dynobj lane(mirror `lower_create` 的 props
/// arm):`Object.preventExtensions({ ... })` 的下游 `__defineGetter__`/
/// `__defineSetter__` guard 只接 DynObj tag(见
/// `torajs-anyvalue/method_call_legacy_accessor.rs`),typed-struct box
/// 出的 Any-boxed value 带 Obj tag → 抛 "legacy accessor methods are
/// not supported on this receiver"(2 test262
/// `Object/prototype/__defineGetter__/define-non-extensible.js` +
/// `Object/prototype/__defineSetter__/define-non-extensible.js`)。走
/// dynobj lane 后 tag == DynObj,define_face → `__torajs_dynobj_define`
/// 内 §10.1.6.3 non-extensible new-key gate 命中 → bun-parity
/// "Attempting to define property on object that is not extensible."。
fn lower_prevent_extensions(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let any_op = if matches!(ctx.ast.get_expr(args[0]), Expr::ObjectLit { .. }) {
        let d = ctx.lower_dynobj_init(args[0]);
        let boxed = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_box,
                vec![Operand::ConstI64(4 /* ANY_HEAP */), d],
            ),
            Type::Any,
            None,
        );
        Operand::Value(boxed)
    } else {
        let obj_op = ctx.lower_expr(args[0]);
        let obj_ty = ctx.operand_ty(&obj_op);
        if matches!(obj_ty, Type::Any) {
            obj_op
        } else {
            ctx.box_to_any_from_expr(args[0], obj_op)
        }
    };
    for a in args.iter().skip(1) {
        let _ = ctx.lower_expr(*a);
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.anyv_prevent_extensions, vec![any_op]),
        Type::Any,
        None,
    );
    // RFC 20260705 owned-result invariant: anyv_prevent_extensions
    // answers the receiver box un-inc'd; the result carries its own
    // ref. Missing pre-fix — a statement-position
    // `Object.preventExtensions(w);` dropped the un-inc'd result and
    // over-released the receiver (use-after-free: the freed wrapper
    // block got recycled by the next alloc, observed as
    // `n.valueOf()` answering `[]` after `Object.keys(n)`).
    // §10.5.4 — a Proxy's preventExtensions trap can refuse or lie,
    // both of which are catchable TypeErrors (RFC 20260823 刀 5).
    ctx.emit_throw_check(None);
    ctx.emit_owned_result_inc(Operand::Value(v), Type::Any);
    Operand::Value(v)
}

/// `Object.isExtensible(obj)` — read `FLAG_NON_EXTENSIBLE`. Primitives
/// route through Any → helper returns `false` per spec.
fn lower_is_extensible(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let obj_op = ctx.lower_expr(args[0]);
    let obj_ty = ctx.operand_ty(&obj_op);
    let any_op = if matches!(obj_ty, Type::Any) {
        obj_op
    } else {
        ctx.box_to_any_from_expr(args[0], obj_op)
    };
    for a in args.iter().skip(1) {
        let _ = ctx.lower_expr(*a);
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.anyv_is_extensible, vec![any_op]),
        Type::Bool,
        None,
    );
    // §10.5.3 step 8 — a Proxy trap that disagrees with its target
    // throws (RFC 20260823 刀 5).
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// `Object.seal(obj)` — set `FLAG_NON_EXTENSIBLE` + `FLAG_SEALED` and
/// clear `configurable` on every DynObj entry. Returns Any-boxed obj.
fn lower_seal(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let obj_op = ctx.lower_expr(args[0]);
    let obj_ty = ctx.operand_ty(&obj_op);
    let any_op = if matches!(obj_ty, Type::Any) {
        obj_op
    } else {
        ctx.box_to_any_from_expr(args[0], obj_op)
    };
    for a in args.iter().skip(1) {
        let _ = ctx.lower_expr(*a);
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.anyv_seal, vec![any_op]),
        Type::Any,
        None,
    );
    // RFC 20260705 owned-result invariant: anyv_seal answers the
    // receiver box un-inc'd; the result carries its own ref.
    ctx.emit_owned_result_inc(Operand::Value(v), Type::Any);
    Operand::Value(v)
}

/// `Object.isSealed(obj)` — `true` iff non-extensible + every property
/// non-configurable. Primitives box to Any → helper returns `true`.
fn lower_is_sealed(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let obj_op = ctx.lower_expr(args[0]);
    let obj_ty = ctx.operand_ty(&obj_op);
    let any_op = if matches!(obj_ty, Type::Any) {
        obj_op
    } else {
        ctx.box_to_any_from_expr(args[0], obj_op)
    };
    for a in args.iter().skip(1) {
        let _ = ctx.lower_expr(*a);
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.anyv_is_sealed, vec![any_op]),
        Type::Bool,
        None,
    );
    Operand::Value(v)
}

/// `Object.{setPrototypeOf,defineProperties,assign}(obj, ...)` — no-op
/// subset returning `obj`; trailing args evaluated for side effects.
/// The chunk-20 `ssa_lower_call_object_assign` arm runs first and
/// claims the structural-copy `Object.assign` path; this is the
/// fallback when that sibling falls through.
/// `Object.setPrototypeOf(O, proto)` — §20.1.2.21 (RFC
/// 20260717-user-proto-chain knife 3). Routes both args through the
/// Any-boxed runtime core (invalid proto / cycle / non-extensible
/// throw there); answers the receiver pass-through per spec.
fn lower_set_prototype_of(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    // An ObjectLit receiver promotes to the dynobj lane (the
    // defineProperty precedent at `lower_define_receiver`): the
    // struct lane has no proto slot, so the runtime core's
    // TAG_DYNOBJ gate silently no-opped the link — neither wrote
    // nor threw (tr answered false where bun answers true). The
    // fresh dynobj is already the owned result, so this shape skips
    // the pass-through inc at the bottom.
    let promoted = matches!(ctx.ast.get_expr(args[0]), Expr::ObjectLit { .. });
    let (obj_op, obj_boxed) = if promoted {
        let dynobj = ctx.lower_dynobj_init(args[0]);
        let boxed = ctx.box_to_any(dynobj);
        (boxed.clone(), boxed)
    } else {
        let op = ctx.lower_expr(args[0]);
        let ty = ctx.operand_ty(&op);
        let boxed = if matches!(ty, Type::Any) {
            op.clone()
        } else {
            ctx.box_to_any_from_expr(args[0], op.clone())
        };
        (op, boxed)
    };
    // A missing proto arg is `undefined` — the runtime core answers
    // the spec TypeError for it.
    let (proto_eid, proto_op, proto_boxed) = match args.get(1) {
        Some(&p_eid) => {
            let p_op = ctx.lower_expr(p_eid);
            let p_ty = ctx.operand_ty(&p_op);
            let boxed = if matches!(p_ty, Type::Any) {
                p_op.clone()
            } else {
                ctx.box_to_any_from_expr(p_eid, p_op.clone())
            };
            (Some(p_eid), Some(p_op), boxed)
        }
        None => {
            let undef = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(1 /* ANY_UNDEF */), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            (None, None, Operand::Value(undef))
        }
    };
    for &a in args.iter().skip(2) {
        let op = ctx.lower_expr(a);
        ctx.release_owned_temp(a, &op);
    }
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.anyv_set_prototype_of,
            vec![obj_boxed, proto_boxed],
        ),
    );
    if let (Some(p_eid), Some(p_op)) = (proto_eid, proto_op) {
        ctx.release_owned_temp(p_eid, &p_op);
    }
    ctx.emit_throw_check(None);
    // ES answers the receiver; the pass-through result carries its
    // own ref (RFC 20260705 owned-result invariant). The promoted
    // literal is ALREADY that ref — inc'ing it too would strand the
    // fresh dynobj one count above its owners.
    if !promoted {
        let obj_ty = ctx.operand_ty(&obj_op);
        ctx.emit_owned_result_inc(obj_op.clone(), obj_ty);
    }
    obj_op
}

fn lower_noop(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let obj_op = ctx.lower_expr(args[0]);
    for a in args.iter().skip(1) {
        let _ = ctx.lower_expr(*a);
    }
    // RFC 20260705 owned-result invariant: the pass-through result
    // (ES answers the target/receiver) carries its own ref — the
    // 542 gate over-release came from the opposite assumption.
    let obj_ty = ctx.operand_ty(&obj_op);
    ctx.emit_owned_result_inc(obj_op.clone(), obj_ty);
    obj_op
}
