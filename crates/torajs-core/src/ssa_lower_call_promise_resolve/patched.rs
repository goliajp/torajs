//! The patched block of a `Promise.resolve` / `Promise.reject` call:
//! the any-lane method call against the boxed ctor cell.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BlockId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The patched block's body — the any-lane method call against the
/// boxed ctor cell, argv packed from the PRE-LOWERED operands (the
/// per-arg bookkeeping mirrors `pack_any_argv`: borrow-shape args
/// take +1 into the box, fresh owned Any temps ride verbatim and
/// release after the call — the runtime borrows argv).
pub(super) fn lower_patched_call(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    name_id: i64,
    args: &[ExprId],
    arg_ops: &[Operand],
) -> Operand {
    let cur_block = ctx.cur_block;
    let recv = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.builtin_ctor_value,
            vec![Operand::ConstI64(10)],
        ),
        Type::Any,
        None,
    );
    let argc = args.len();
    let argv = ctx.f.append_inst(
        BlockId(0),
        InstKind::AllocaBytes((argc.max(1) * 8) as u64),
        Type::Ptr,
        Some("__pspatch_argv"),
    );
    let mut drops: Vec<Operand> = Vec::with_capacity(argc);
    for (i, (&aid, op)) in args.iter().zip(arg_ops).enumerate() {
        let ty = ctx.operand_ty(op);
        let slot_val = if ty == Type::Any {
            if ctx.expr_is_fresh_owned(aid) {
                drops.push(*op);
            }
            *op
        } else {
            // Same question the Any arm above already asks the
            // shared way — and the shared answer reads through the
            // casts a roster of AST shapes cannot (rotation 572).
            let is_borrow = !ctx.expr_is_fresh_owned(aid)
                || matches!(
                    ctx.ast.get_expr(ctx.peel_as_wrappers(aid)),
                    Expr::Regex { .. }
                );
            let boxed = ctx.box_owned_from_expr(aid, *op, is_borrow);
            drops.push(boxed);
            boxed
        };
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Store(slot_val, Operand::Value(argv), (i * 8) as u64),
        );
    }
    let mid = torajs_rc::any_method_id(method);
    let name_str = ctx.intern_string_literal(method);
    let cur_block = ctx.cur_block;
    let result = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_method_call,
            vec![
                Operand::Value(recv),
                Operand::ConstI64(mid),
                Operand::Value(name_str),
                Operand::ConstI64(0),
                Operand::ConstPtrNull,
                Operand::Value(argv),
                Operand::ConstI64(argc as i64),
            ],
        ),
        Type::Any,
        None,
    );
    for d in drops {
        ctx.emit_drop_value(d, Type::Any);
    }
    // The override can throw (user code) — propagate with the owned
    // result released; the ctor-cell box is a bit-encode over an
    // immortal static cell, no release rides it.
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    let cur_block = ctx.cur_block;
    let out = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.promise_patched_result,
            vec![Operand::Value(result), Operand::ConstI64(name_id)],
        ),
        Type::Promise,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(out)
}
