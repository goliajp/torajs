//! `Iterator.concat(...items)` lowering (proposal-iterator-sequencing)
//! — RFC 20260730-iterator-global 刀 5a.
//!
//! Statics wedge in the Iterator.from shape, variadic: every argument
//! becomes an owned Any slot in a fresh `Array<Any>` whose reference
//! TRANSFERS to `__torajs_iterator_concat` (the kernel's minted cell
//! takes it, or its refusal path drops it — the lowering never emits
//! a release for the pack). Return surfaces as `Type::Any` (a lazy
//! kind-CONCAT IterHelper cell).

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
    if name != "concat" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id) else {
        return None;
    };
    if ns != "Iterator" {
        return None;
    }
    // Fresh Array<Any> pack — one owned slot per argument.
    let n_op = Operand::ConstI64(args.len() as i64);
    let mut arr = Operand::Value(ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc_any, vec![n_op]),
        Type::Ptr,
        None,
    ));
    for &a in args.iter() {
        let raw = ctx.lower_expr(a);
        let ty = ctx.operand_ty(&raw);
        // Each slot needs an OWNED Any: transfers keep their fresh
        // reference, borrows (ident reads, member reads) take +1 so
        // the source binding keeps its own stake.
        let av = match ty {
            Type::Any => {
                if !ctx.expr_transfers_ownership(a) {
                    ctx.emit_rc_inc(raw.clone());
                }
                raw
            }
            Type::Arr(_) => {
                ctx.emit_arr_mark_kind(&raw);
                if !ctx.expr_transfers_ownership(a) {
                    ctx.emit_rc_inc(raw.clone());
                }
                let as_i64 =
                    ctx.f
                        .append_inst(ctx.cur_block, InstKind::PtrToInt(raw), Type::I64, None);
                let as_any = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::IntToPtr(Operand::Value(as_i64)),
                    Type::Any,
                    None,
                );
                Operand::Value(as_any)
            }
            _ => {
                if !ctx.expr_transfers_ownership(a) && ty.is_refcounted() {
                    ctx.emit_rc_inc(raw.clone());
                }
                ctx.box_to_any(raw)
            }
        };
        let tag = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![av.clone()]),
            Type::I64,
            None,
        );
        let val = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_unbox_value, vec![av]),
            Type::I64,
            None,
        );
        arr = Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_push_any,
                vec![arr, Operand::Value(tag), Operand::Value(val)],
            ),
            Type::Ptr,
            None,
        ));
    }
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.iterator_concat, vec![arr]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Some(Operand::Value(result))
}
