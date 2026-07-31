//! `Iterator.zip(iterables, options)` lowering
//! (proposal-joint-iteration) — RFC 20260730-iterator-global 刀 5b.
//!
//! Statics wedge in the Iterator.from shape, binary: both operands
//! box to Any and pass BORROWED to `__torajs_iterator_zip` (the
//! kernel opens every input iterator eagerly and mints a lazy
//! kind-ZIP IterHelper cell; refusals leave a pending throw). An
//! absent options argument passes the undefined immediate.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj: ns_id, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if name != "zip" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id) else {
        return None;
    };
    if ns != "Iterator" {
        return None;
    }
    let mut ops: Vec<(Operand, bool, Option<ExprId>)> = Vec::with_capacity(2);
    for slot in 0..2usize {
        if slot >= args.len() {
            // Absent argument — the undefined immediate (kernel
            // treats it per GetOptionsObject / the step-1 gate).
            let undef = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            ops.push((Operand::Value(undef), false, None));
            continue;
        }
        let eid = args[slot];
        let raw = ctx.lower_expr(eid);
        let ty = ctx.operand_ty(&raw);
        let (op, we_boxed) = match ty {
            Type::Any => (raw, false),
            Type::Arr(_) => {
                ctx.emit_arr_mark_kind(&raw);
                let as_i64 =
                    ctx.f
                        .append_inst(ctx.cur_block, InstKind::PtrToInt(raw), Type::I64, None);
                let as_any = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::IntToPtr(Operand::Value(as_i64)),
                    Type::Any,
                    None,
                );
                (Operand::Value(as_any), false)
            }
            _ => {
                if !ctx.expr_transfers_ownership(eid) && ty.is_refcounted() {
                    ctx.emit_rc_inc(raw.clone());
                }
                (ctx.box_to_any(raw), true)
            }
        };
        ops.push((op, we_boxed, Some(eid)));
    }
    // Trailing args beyond the spec's two — lower for side effects.
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let argv: Vec<Operand> = ops.iter().map(|(op, _, _)| op.clone()).collect();
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.iterator_zip, argv),
        Type::Any,
        None,
    );
    // Kernel borrows both values (it derives its own owned columns).
    for (op, we_boxed, eid) in ops {
        if we_boxed {
            ctx.emit_drop_value(op, Type::Any);
        } else if let Some(eid) = eid {
            ctx.release_owned_temp(eid, &op);
        }
    }
    ctx.emit_throw_check(None);
    Some(Operand::Value(result))
}
