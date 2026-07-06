//! `fs_promises.<method>` async wrappers (`readFile` / `writeFile` /
//! `appendFile` / `unlink` / `mkdir` / `exists` / `readdir`) pulled out
//! of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-26 of the `Expr::Call` god-arm decomp (chunks 1-25 = Arr
//! higher-order + Map dispatch + Set dispatch + Arr.push + Number
//! instance methods + bare-name globals + Str regex methods + Number
//! namespace + Array.from + Arr predicate iter + Arr.flatMap +
//! Object.entries + fn-indirect + Number/String/Boolean coercion +
//! universal methods + closure-local + Object.values + Object.keys +
//! Object.getPrototypeOf + Object.assign + Bun runtime cluster +
//! Reflect.get + Symbol.for/keyFor + Object.hasOwn/Reflect.has +
//! class-synth register globals).
//!
//! T-18.a (v0.5.0) — each method routes to the matching `fs_<m>_sync`
//! intrinsic, then wraps the sync result in a fulfilled Promise so the
//! user-visible `Promise<T>` contract holds. MVP "synchronous-then-
//! resolve" — real I/O suspension awaits the T-16 state-machine
//! async/await pass. Dispatch by sync return type:
//! - `Type::Str` / `Type::Arr(_)` → `promise_alloc_fulfilled_heap`
//!   (Promise takes ownership of the heap value's rc).
//! - `Type::Bool` → `promise_alloc_fulfilled` after `coerce_bool_to_i64`
//!   pack.
//! - `Type::Void` → `promise_alloc_fulfilled` with `ConstI64(0)`
//!   sentinel.
//!
//! Returns `Some(op)` on hit; `None` on miss (non-Member callee,
//! non-`fs_promises` namespace, or non-allowlisted method) so the
//! caller falls through to subsequent arms.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};

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
    if ns != "fs_promises" {
        return None;
    }
    if !matches!(
        method.as_str(),
        "readFile" | "writeFile" | "appendFile" | "unlink" | "mkdir" | "exists" | "readdir"
    ) {
        return None;
    }

    // The fs helpers borrow every Str arg (bytes copied to a stack
    // buf, no rc traffic), so Ident args keep their stake and their
    // scope drop — the old consume path orphaned it (RFC 20260705
    // ledger #3). Owned temps release after the sync call below.
    let arg_ops: Vec<Operand> = args.iter().map(|a| ctx.lower_expr(*a)).collect();
    let (sync_fid, sync_ret_ty) = match method.as_str() {
        "readFile" => (ctx.intrinsics.fs_read_file_sync, Type::Str),
        "writeFile" => (ctx.intrinsics.fs_write_file_sync, Type::Void),
        "appendFile" => (ctx.intrinsics.fs_append_file_sync, Type::Void),
        "unlink" => (ctx.intrinsics.fs_unlink_sync, Type::Void),
        "mkdir" => (ctx.intrinsics.fs_mkdir_sync, Type::Void),
        "exists" => (ctx.intrinsics.fs_exists_sync, Type::Bool),
        "readdir" => {
            let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
            (ctx.intrinsics.fs_readdir_sync, Type::Arr(arr_id))
        }
        _ => unreachable!(),
    };
    let cur_block = ctx.cur_block;
    let sync_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(sync_fid, arg_ops.clone()),
        sync_ret_ty,
        None,
    );
    for (a, op) in args.iter().zip(arg_ops.iter()) {
        ctx.release_owned_temp(*a, op);
    }
    // Wrap sync result. Heap variants (Str / Arr) hand ownership to the
    // Promise; primitive (Bool / Void) packs into i64.
    let (promise_alloc_fid, value_op) = match sync_ret_ty {
        Type::Str | Type::Arr(_) => (
            ctx.intrinsics.promise_alloc_fulfilled_heap,
            Operand::Value(sync_v),
        ),
        Type::Bool => (
            ctx.intrinsics.promise_alloc_fulfilled,
            ctx.coerce_bool_to_i64(Operand::Value(sync_v)),
        ),
        Type::Void => (ctx.intrinsics.promise_alloc_fulfilled, Operand::ConstI64(0)),
        _ => unreachable!(),
    };
    let p_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(promise_alloc_fid, vec![value_op]),
        Type::Promise,
        None,
    );
    Some(Operand::Value(p_v))
}
