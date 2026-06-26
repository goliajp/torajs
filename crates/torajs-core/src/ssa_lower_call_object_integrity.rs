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
    if ns != "Object" {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    match method.as_str() {
        "freeze" | "isFrozen" => Some(lower_freeze_or_is_frozen(ctx, method.as_str(), args)),
        "create" => Some(lower_create(ctx, args)),
        "preventExtensions" => Some(lower_prevent_extensions(ctx, args)),
        "isExtensible" => Some(lower_is_extensible(ctx, args)),
        "seal" => Some(lower_seal(ctx, args)),
        "isSealed" => Some(lower_is_sealed(ctx, args)),
        "setPrototypeOf" | "defineProperties" | "assign" => Some(lower_noop(ctx, args)),
        _ => None,
    }
}

/// `Object.freeze(obj)` / `Object.isFrozen(obj)` — primitive short
/// circuit + heap-header bit set/read. Any-typed receivers use the
/// NaN-box-aware `obj_is_frozen_any` for isFrozen.
fn lower_freeze_or_is_frozen(ctx: &mut LowerCtx<'_>, method: &str, args: &[ExprId]) -> Operand {
    let arg_op = ctx.lower_expr(args[0]);
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let arg_ty = ctx.operand_ty(&arg_op);
    let is_primitive = matches!(arg_ty, Type::I64 | Type::F64 | Type::Bool);
    if is_primitive {
        return if method == "freeze" {
            arg_op
        } else {
            Operand::ConstBool(true)
        };
    }
    let (fid, ret_ty) = if method == "freeze" {
        (ctx.intrinsics.obj_freeze, arg_ty)
    } else if arg_ty == Type::Any {
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

/// `Object.create(proto[, descriptors])` — allocate fresh dynobj-
/// backed Any-box. Args evaluated for side effects then discarded.
fn lower_create(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    for a in args {
        let _ = ctx.lower_expr(*a);
    }
    let cur_block = ctx.cur_block;
    let dynobj = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.dynobj_alloc, vec![]),
        Type::Ptr,
        None,
    );
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

/// `Object.preventExtensions(obj)` — flip `FLAG_NON_EXTENSIBLE`. Boxes
/// typed receivers to Any so the helper has a single ABI.
fn lower_prevent_extensions(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
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
        InstKind::Call(ctx.intrinsics.anyv_prevent_extensions, vec![any_op]),
        Type::Any,
        None,
    );
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
fn lower_noop(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let obj_op = ctx.lower_expr(args[0]);
    for a in args.iter().skip(1) {
        let _ = ctx.lower_expr(*a);
    }
    obj_op
}
