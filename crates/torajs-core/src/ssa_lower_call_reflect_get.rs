//! `Reflect.get(target, key)` compile-time fold pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-22 of the `Expr::Call` god-arm decomp (chunks 1-21 = Arr
//! higher-order + Map dispatch + Set dispatch + Arr.push + Number
//! instance methods + bare-name globals + Str regex methods + Number
//! namespace + Array.from + Arr predicate iter + Arr.flatMap +
//! Object.entries + fn-indirect + Number/String/Boolean coercion +
//! universal methods + closure-local + Object.values + Object.keys +
//! Object.getPrototypeOf + Object.assign + Bun runtime cluster).
//!
//! ES §28.1.6 typed subset: when the target lowers to `Type::Obj(sid)`
//! and the key arg is a string literal, the call folds to a struct
//! field load + box-to-Any. Missing key boxes to ANY_UNDEF=5. Other
//! target shapes (dynamic key, non-struct target) are deferred — the
//! arm panics rather than falling through so the caller doesn't reach
//! the generic Call path, mirroring the inline behaviour before extract.
//!
//! Trailing args past `args[1]` are spec-lowered for side-effect
//! parity (`step()`-style exprs must still fire) and discarded; the
//! receiver-arg (`args[2]` per ES) is typecheck-dropped upstream by
//! `check.rs` S257.
//!
//! Refcount discipline: refcounted field types `emit_rc_inc` on the
//! borrowed load before transferring ownership into the Any box —
//! caller will drop the Any box, so the underlying field needs its
//! own rc tick to stay alive.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "get" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Reflect" {
        return None;
    }
    if args.len() < 2 {
        return None;
    }
    let Expr::String(key_lit) = ctx.ast.get_expr(args[1]) else {
        return None;
    };
    let key = key_lit.clone();
    let obj_op = ctx.lower_expr(args[0]);
    // ES §28.1.6 receiver arg + trailing silently ignored in the typed-
    // struct subset. check.rs S257 typecheck-drops them; mirror lower-
    // and-drop here so step()-style side-effect exprs fire (S272 idiom).
    // Placed after obj-lower / before dispatch so trailing eval is
    // exactly-once across the typed-struct + missing-key + panic paths.
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let obj_ty = ctx.operand_ty(&obj_op);
    let Type::Obj(sid) = obj_ty else {
        panic!(
            "ssa-lower: Reflect.get requires a typed-struct target with a literal string key; got target type {obj_ty:?}"
        );
    };
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    if let Some((idx, (_, fty))) = layout
        .iter()
        .enumerate()
        .find(|(_, (n, _))| n == &key)
        .map(|(i, (n, t))| (i, (n.clone(), *t)))
    {
        let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
        let cur_block = ctx.cur_block;
        let field_val =
            ctx.f
                .append_inst(cur_block, InstKind::Load(fty, obj_op, offset), fty, None);
        // borrow semantics — caller will drop the boxed Any, so rc_inc
        // refcounted fields before transferring ownership into the box.
        if fty.is_refcounted() {
            ctx.emit_rc_inc(Operand::Value(field_val));
        }
        return Some(ctx.box_to_any(Operand::Value(field_val)));
    }
    // Key absent from layout → undefined per spec.
    let cur_block = ctx.cur_block;
    let any_box = ctx.intrinsics.any_box;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(any_box, vec![Operand::ConstI64(5), Operand::ConstI64(0)]),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}
