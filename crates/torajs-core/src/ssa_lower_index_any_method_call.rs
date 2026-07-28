//! `recv[key](args…)` where the receiver types as `any` — the
//! index-call receiver-semantics arm (RFC 20260728-gen-forof-
//! yieldstar F0b).
//!
//! ES §13.3.6.2 EvaluateCall: a property-reference callee makes
//! this a METHOD call — thisValue is the base. Before this arm the
//! Index callee (typing Any) fell into the bare any-call layer,
//! which read the property as a value and called it receiverless:
//! a builtin reified cell hit its this-undefined TypeError and a
//! user objlit method ran with `this` undefined.
//!
//! The lowering mirrors the named-form sibling
//! ([`crate::ssa_lower_any_method_call`]): receiver boxes at the
//! any-lane boundary, the key boxes to an Any the runtime
//! ToPropertyKey-dispatches (Str/short-str name → by-name dispatch,
//! Symbol → symbol face, everything else → ToString), args pack via
//! the shared `pack_any_argv` ledger, and an Ident receiver's
//! variable slot rides along for growth-relocating methods.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_any_method_call::pack_any_argv;

/// Try to lower `callee(args…)` as an any-receiver index-keyed
/// method call. Returns `None` unless the callee is an Index read
/// off an `any`-typed object.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Index { obj, index } = ctx.ast.get_expr(callee) else {
        return None;
    };
    let (obj, index) = (*obj, *index);
    if !matches!(ctx.expr_types.get(&obj), Some(crate::check::Type::Any)) {
        return None;
    }

    let recv = ctx.lower_expr(obj);
    // An `xs as any` receiver reaches here as a typed SSA value (the
    // As cast is a pass-through for heap values) — box at this
    // any-lane boundary; already-Any receivers pass through
    // (borrow-shaped, the runtime only borrows).
    let recv = if matches!(ctx.operand_ty(&recv), Type::Any) {
        recv
    } else {
        ctx.box_to_any(recv)
    };
    // Ident receivers ride their variable slot along so
    // growth-relocating methods (push) write the fresh block pointer
    // back — same two shapes as the named-form arm.
    let recv_slot = if let Expr::Ident(n) = ctx.ast.get_expr(obj) {
        if let Some(info) = ctx.locals.get(n) {
            Operand::Value(info.slot)
        } else if ctx.globals.contains_key(n) {
            let name = n.clone();
            let gref = ctx
                .f
                .append_inst(ctx.cur_block, InstKind::GlobalRef(name), Type::Ptr, None);
            Operand::Value(gref)
        } else {
            Operand::ConstPtrNull
        }
    } else {
        Operand::ConstPtrNull
    };

    // The key boxes to an Any the runtime dispatches on. Ledger per
    // the chunk-496 three-shape rule: a borrowed refcounted operand
    // rc-incs before the TRANSFER-shaped box, temps hand their
    // reference to the box, already-Any keys pass through borrowed.
    let key_is_borrow = matches!(
        ctx.ast.get_expr(index),
        Expr::Ident(_) | Expr::Member { .. } | Expr::Regex { .. }
    );
    let key_raw = ctx.lower_expr(index);
    let key_ty = ctx.operand_ty(&key_raw);
    let (key, key_boxed) = if key_ty == Type::Any {
        (key_raw, false)
    } else {
        if key_is_borrow && key_ty.is_refcounted() {
            ctx.emit_rc_inc(key_raw.clone());
        }
        (ctx.box_to_any_from_expr(index, key_raw), true)
    };

    let (argv, boxed_slots) = pack_any_argv(ctx, args);

    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_index_method_call,
            vec![
                recv.clone(),
                key.clone(),
                recv_slot,
                Operand::Value(argv),
                Operand::ConstI64(args.len() as i64),
            ],
        ),
        Type::Any,
        None,
    );
    // Release the boxes' references BEFORE the throw check — the
    // runtime borrowed argv and the key; per-method glue inc'd
    // whatever it stored.
    for slot in boxed_slots.into_iter().flatten() {
        ctx.emit_drop_value(slot, Type::Any);
    }
    if key_boxed {
        ctx.emit_drop_value(key, Type::Any);
    } else {
        // An already-Any key minted by a Call is an owned temp the
        // runtime only borrowed (borrow shapes self-gate false).
        ctx.release_owned_temp(index, &key);
    }
    // A Call-shaped receiver is an owned Any temp the runtime only
    // borrowed — mirrors the named-form arm's receiver account.
    ctx.release_owned_temp(obj, &recv);
    // The result is an OWNED Any already in hand — the throw path
    // must release it.
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    Some(Operand::Value(result))
}
