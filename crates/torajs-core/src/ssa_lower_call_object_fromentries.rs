//! `Object.fromEntries(entries)` with a RUNTIME entries array
//! (chunk 693, T-09) — the dynamic complement to the two
//! compile-time lanes (static pairs literal → ObjectLit fold,
//! chunk 690 `ast::fold_fromentries`; struct-annotated let
//! fast-path, T-09.c `ssa_lower_stmt_let_decl_fromentries`).
//!
//! Both surviving receiver shapes route through one runtime
//! walker, `__torajs_anyv_from_entries` (torajs-meta), which is
//! kind-aware per slot and answers a fresh dynobj as a NaN-box
//! AnyValue — downstream member reads / prints ride the existing
//! dynobj lanes:
//! - `Type::Arr(_)` — a raw Arr pointer already satisfies the
//!   cell encoding; PtrToInt→IntToPtr exposes it as Any bits
//!   (the any_box writeback idiom).
//! - `Type::Map` / `Type::Set` (chunk 694) — same raw-pointer
//!   exposure (the hash cell is self-describing, no kind stamp);
//!   the walker rides `__torajs_map_iter_next` over the shared
//!   storage.
//! - `Type::Any` — passed through; undefined / null / non-array
//!   cells throw a catchable TypeError inside the helper.
//!
//! Everything else (structs, primitives) falls through to the
//! resolve_callee loud panic, unchanged.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "fromEntries" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Object" {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    // Peek the receiver type without committing: only Arr / Any
    // shapes take this arm (the let fast-path consumed the
    // struct-annotated shape before lower_expr reaches here).
    let arg_op = ctx.lower_expr(args[0]);
    // S309 — lower-and-drop trailing args (spec ignores them; their
    // evaluation side effects keep L-to-R order).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let arg_ty = ctx.operand_ty(&arg_op);
    let any_bits = match arg_ty {
        Type::Any => arg_op,
        Type::Arr(_) | Type::Map | Type::Set => {
            if matches!(arg_ty, Type::Arr(_)) {
                // Stamp the elem kind (recursively — the chain covers
                // the inner pair arrays) so the walker's kind-aware
                // borrowed reads see a self-describing block instead
                // of UNSET. Hash cells need no stamp (the heap
                // header's type_tag routes the walker).
                ctx.emit_arr_mark_kind(&arg_op);
            }
            let as_i64 =
                ctx.f
                    .append_inst(ctx.cur_block, InstKind::PtrToInt(arg_op), Type::I64, None);
            let as_any = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::IntToPtr(Operand::Value(as_i64)),
                Type::Any,
                None,
            );
            Operand::Value(as_any)
        }
        other => panic!("ssa-lower: unsupported member call shape: fromEntries on {other:?}"),
    };
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.anyv_from_entries, vec![any_bits]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Some(Operand::Value(v))
}
