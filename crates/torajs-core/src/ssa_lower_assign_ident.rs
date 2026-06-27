//! `Expr::Assign { target: Expr::Ident(name), value }` lowering pulled
//! out of [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Assign`
//! match arm as chunk-79a of the decomp (chunks 1-78 = ... + `Expr::New`
//! 6-class ctor cluster).
//!
//! Two assignment paths:
//!
//! 1. **K.3 module-level data global** — `globals.contains(name)`:
//!    `GlobalRef + Store` to the slot pointer. Primitive Copy types
//!    have no old-value drop dance; K.4 refcount globals (Str) are
//!    rejected loudly (mutable refcount globals are a follow-up).
//!    P11.2-A1 type-check rejects silent `f64 → i64` slot stores;
//!    permissible coercions:
//!    - `slot F64 + value I64` → `coerce_to_f64`
//!    - `slot {I64|F64} + value Any` → `coerce_any_to_number` (catch-
//!      block narrow)
//!    - `slot I64 + value I32` / `slot I32 + value I64`
//!    - `slot Ptr || value Ptr` (free)
//! 2. **Local binding** — `locals.contains(name)`:
//!    Lower rhs FIRST (`s = s + "x"` consumes lhs binding;
//!    `consume_if_ident` after RHS lower marks src moved). For
//!    refcounted borrow rhs (Member / Index / unmoved Ident),
//!    `emit_rc_inc` since lhs + rhs share ownership. Type-check
//!    with permissible coercions:
//!    - `slot Any + value !Any` → `box_to_any`
//!    - `slot F64 + value I64` → `coerce_to_f64`
//!    - `slot {I64|F64} + value Any` → `coerce_any_to_number`
//!    - `slot Str + value Any` → `coerce_to_str` (catch-block
//!      `saved = e` shape)
//!    - `slot I64 + value I32` / `slot I32 + value I64` / Ptr-free
//!    Drop old slot value if non-Copy AND not moved out by RHS
//!    consume. Store new value; clear `moved` flag so subsequent
//!    reads work and end-of-fn drop fires.
//!
//! Returns `Operand` directly (TS assignment-as-expression yields
//! the assigned value).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, name: String, value: ExprId) -> Operand {
    if ctx.locals.get(&name).is_none()
        && let Some(slot_ty) = ctx.globals.get(&name).copied()
    {
        return lower_global_assign(ctx, name, slot_ty, value);
    }
    lower_local_assign(ctx, name, value)
}

fn lower_global_assign(
    ctx: &mut LowerCtx<'_>,
    name: String,
    slot_ty: Type,
    value: ExprId,
) -> Operand {
    if slot_ty.is_refcounted() {
        panic!(
            "ssa-lower: assignment to refcount global `{name}` is not yet supported (K.4 ships read-only Str globals; mutable refcount globals are a follow-up)"
        );
    }
    let v = ctx.lower_expr(value);
    let v_ty = ctx.operand_ty(&v);
    if !global_coercion_compatible(slot_ty, v_ty) {
        panic!(
            "ssa-lower: assignment to global `{name}` mismatch — slot is {slot_ty:?} but value is {v_ty:?}; use `>>` for integer divide or annotate the slot as the appropriate numeric width",
        );
    }
    let v = coerce_for_global(ctx, slot_ty, v_ty, v);
    let cur_block = ctx.cur_block;
    let ptr = ctx
        .f
        .append_inst(cur_block, InstKind::GlobalRef(name), Type::Ptr, None);
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Store(v, Operand::Value(ptr), 0));
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Load(slot_ty, Operand::Value(ptr), 0),
        slot_ty,
        None,
    );
    Operand::Value(r)
}

fn global_coercion_compatible(slot_ty: Type, v_ty: Type) -> bool {
    v_ty == slot_ty
        || (slot_ty == Type::F64 && v_ty == Type::I64)
        || (slot_ty == Type::I64 && v_ty == Type::I32)
        || (slot_ty == Type::I32 && v_ty == Type::I64)
        || slot_ty == Type::Ptr
        || v_ty == Type::Ptr
        || (matches!(slot_ty, Type::I64 | Type::F64) && v_ty == Type::Any)
}

fn coerce_for_global(ctx: &mut LowerCtx<'_>, slot_ty: Type, v_ty: Type, v: Operand) -> Operand {
    if slot_ty == Type::F64 && v_ty == Type::I64 {
        ctx.coerce_to_f64(v)
    } else if matches!(slot_ty, Type::I64 | Type::F64) && v_ty == Type::Any {
        ctx.coerce_any_to_number(v, slot_ty)
    } else {
        v
    }
}

fn lower_local_assign(ctx: &mut LowerCtx<'_>, name: String, value: ExprId) -> Operand {
    let snapshot = match ctx.locals.get(&name) {
        Some(i) => *i,
        None => panic!("ssa-lower: assign to unknown ident `{name}`"),
    };
    let v = ctx.lower_expr(value);
    ctx.consume_if_ident(value);
    apply_borrow_rc_inc(ctx, &v, value);
    let v_ty = ctx.operand_ty(&v);
    check_local_coercion(ctx, &name, snapshot.ty, v_ty);
    let v = coerce_for_local(ctx, snapshot.ty, v_ty, v);
    let post_rhs = *ctx.locals.get(&name).unwrap_or(&snapshot);
    if !snapshot.ty.is_copy() && !post_rhs.moved {
        let cur_block = ctx.cur_block;
        let old = ctx.f.append_inst(
            cur_block,
            InstKind::Load(snapshot.ty, Operand::Value(snapshot.slot), 0),
            snapshot.ty,
            None,
        );
        ctx.emit_drop_value(Operand::Value(old), snapshot.ty);
    }
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(v, Operand::Value(snapshot.slot), 0),
    );
    if let Some(info) = ctx.locals.get_mut(&name) {
        info.moved = false;
    }
    v
}

fn apply_borrow_rc_inc(ctx: &mut LowerCtx<'_>, v: &Operand, value: ExprId) {
    let v_is_refcounted = ctx.operand_ty(v).is_refcounted();
    if !v_is_refcounted {
        return;
    }
    let needs_inc = match ctx.ast.get_expr(value) {
        Expr::Member { .. } | Expr::Index { .. } => true,
        Expr::Ident(src) => ctx.locals.get(src).map(|info| !info.moved).unwrap_or(false),
        _ => false,
    };
    if needs_inc {
        ctx.emit_rc_inc(*v);
    }
}

fn check_local_coercion(ctx: &LowerCtx<'_>, name: &str, snap_ty: Type, v_ty: Type) {
    if snap_ty == Type::Any && v_ty != Type::Any {
        return;
    }
    let direct_compatible = v_ty == snap_ty
        || (snap_ty == Type::F64 && v_ty == Type::I64)
        || (matches!(snap_ty, Type::I64 | Type::F64) && v_ty == Type::Any)
        || (snap_ty == Type::Str && v_ty == Type::Any);
    if direct_compatible {
        return;
    }
    let int_or_ptr_lax = (snap_ty == Type::I64 && v_ty == Type::I32)
        || (snap_ty == Type::I32 && v_ty == Type::I64)
        || snap_ty == Type::Ptr
        || v_ty == Type::Ptr;
    if int_or_ptr_lax {
        return;
    }
    let _ = ctx;
    panic!(
        "ssa-lower: assignment to `{name}` mismatch — slot is {snap_ty:?} but value is {v_ty:?}; use `>>` for integer divide or annotate the slot as the appropriate numeric width",
    );
}

fn coerce_for_local(ctx: &mut LowerCtx<'_>, snap_ty: Type, v_ty: Type, v: Operand) -> Operand {
    if snap_ty == Type::Any && v_ty != Type::Any {
        ctx.box_to_any(v)
    } else if snap_ty == Type::F64 && v_ty == Type::I64 {
        ctx.coerce_to_f64(v)
    } else if matches!(snap_ty, Type::I64 | Type::F64) && v_ty == Type::Any {
        ctx.coerce_any_to_number(v, snap_ty)
    } else if snap_ty == Type::Str && v_ty == Type::Any {
        ctx.coerce_to_str(v, Type::Any)
    } else {
        v
    }
}
