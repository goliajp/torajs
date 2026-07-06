//! Bun runtime cluster (`Bun.file` / `Bun.gc` / `fetch` /
//! `<response>.text()` / `<bunfile>.text() / .exists()` /
//! `__torajs_weakref_create`) pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-21 of the `Expr::Call` god-arm decomp (chunks 1-20 = Arr
//! higher-order + Map dispatch + Set dispatch + Arr.push + Number
//! instance methods + bare-name globals + Str regex methods + Number
//! namespace + Array.from + Arr predicate iter + Arr.flatMap +
//! Object.entries + fn-indirect + Number/String/Boolean coercion +
//! universal methods + closure-local + Object.values + Object.keys +
//! Object.getPrototypeOf + Object.assign).
//!
//! Six small arms share a common shape: identify a Bun-flavoured call
//! site (either by global namespace `Bun.<m>`, the bare `fetch` /
//! `__torajs_weakref_create` globals, or by static type / syntactic
//! receiver shape) and emit the corresponding intrinsic call. Each
//! returns `Some(op)` on hit, `None` on miss so the caller falls
//! through to the next arm or generic Call lowering.
//!
//! Semantics summary (per tr's bun-parity slice):
//! - `Bun.file(path)` — SSA-level no-op passthrough; the BunFile
//!   handle is the path Str. Method dispatch on the handle
//!   (`.text()` / `.exists()`) uses the path directly.
//! - `Bun.gc(synchronous)` — fires the Bacon-Rajan cycle collector
//!   (`cycle_collect`). The bool arg is ignored (JSC gates concurrent
//!   GC; tr runs the collector synchronously).
//! - `fetch(url)` — synchronous wire of `__torajs_fetch_sync` returning
//!   a `Type::Ptr` Response*, wrapped in a fulfilled Promise (heap
//!   variant).
//! - `<response>.text()` — typed on `Type::Object("Response")`. Loads
//!   the body Str at offset 16, bumps its rc, wraps in fulfilled
//!   Promise.
//! - `<bunfile>.text() / .exists()` — receiver shape `Bun.file(...)`.
//!   `.text()` routes through `fs_read_file_sync`, `.exists()` through
//!   `fs_exists_sync` (bool → i64 coerced before primitive-Promise wrap).
//! - `__torajs_weakref_create(target)` — observation only, target NOT
//!   consumed. The registry observes the ptr; the user's binding
//!   continues normal drop semantics so the surrounding scope's drop
//!   walk fires `weakref_target_dying`.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    match ctx.ast.get_expr(callee) {
        Expr::Ident(n) => {
            if n == "fetch" {
                return try_lower_fetch(ctx, args);
            }
            if n == "__torajs_weakref_create" {
                return try_lower_weakref_create(ctx, args);
            }
            None
        }
        Expr::Member { obj, name } => {
            let recv_id = *obj;
            let m_name = name.clone();
            // Bun.<method>(...)
            if let Expr::Ident(ns) = ctx.ast.get_expr(recv_id)
                && ns == "Bun"
            {
                if m_name == "file" {
                    return try_lower_bun_file(ctx, args);
                }
                if m_name == "gc" {
                    return try_lower_bun_gc(ctx, args);
                }
            }
            // <response>.text() — typed Response receiver.
            if m_name == "text"
                && args.is_empty()
                && let Some(op) = try_lower_response_text(ctx, recv_id)
            {
                return Some(op);
            }
            // <bunfile>.text() / .exists() — syntactic receiver shape.
            if (m_name == "text" || m_name == "exists") && args.is_empty() {
                return try_lower_bunfile_method(ctx, recv_id, &m_name);
            }
            None
        }
        _ => None,
    }
}

/// `Bun.file(path)` — pass the path Str straight through. Caller's
/// `arg_op` is the BunFile handle. Identity pass-through: the result
/// IS the arg value under the owned-result invariant, so a borrow-
/// shaped path shares (+1, source keeps its stake — the old consume
/// stole it, String(str)-station twin); owned temps transfer.
fn try_lower_bun_file(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let arg_op = ctx.lower_expr(args[0]);
    if !ctx.expr_transfers_ownership(args[0]) {
        ctx.emit_rc_inc(arg_op.clone());
    }
    Some(arg_op)
}

/// `Bun.gc(synchronous)` — fire the Bacon-Rajan cycle collector. The
/// bool arg is lowered for side-effect parity but ignored at the
/// intrinsic level (tr is always synchronous).
fn try_lower_bun_gc(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    for a in args.iter() {
        let _ = ctx.lower_expr(*a);
    }
    let cur_block = ctx.cur_block;
    let cycle_collect = ctx.intrinsics.cycle_collect;
    ctx.f
        .append_void(cur_block, InstKind::Call(cycle_collect, vec![]));
    Some(Operand::ConstI64(0))
}

/// `fetch(url)` — synchronous wire of `__torajs_fetch_sync` returning
/// a heap `Response*` wrapped in a fulfilled Promise.
fn try_lower_fetch(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let url_op = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    let fetch_sync = ctx.intrinsics.fetch_sync;
    let promise_alloc_fulfilled_heap = ctx.intrinsics.promise_alloc_fulfilled_heap;
    let resp_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fetch_sync, vec![url_op.clone()]),
        Type::Ptr,
        None,
    );
    // `fetch_sync` borrows the url (reads into a CString), so an Ident
    // arg keeps its stake and its scope drop; owned temps release here.
    ctx.release_owned_temp(args[0], &url_op);
    let p_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(promise_alloc_fulfilled_heap, vec![Operand::Value(resp_v)]),
        Type::Promise,
        None,
    );
    Some(Operand::Value(p_v))
}

/// `<response>.text()` — typed on `Type::Object("Response")`. Loads
/// body Str at offset 16, rc-bumps, wraps in fulfilled Promise.
/// Returns `None` if the receiver is not typed Response so a sibling
/// arm (BunFile) can claim the call.
fn try_lower_response_text(ctx: &mut LowerCtx<'_>, resp_id: ExprId) -> Option<Operand> {
    if !matches!(
        ctx.expr_types.get(&resp_id),
        Some(crate::check::Type::Object("Response"))
    ) {
        return None;
    }
    let resp_op = ctx.lower_expr(resp_id);
    let cur_block = ctx.cur_block;
    let promise_alloc_fulfilled_heap = ctx.intrinsics.promise_alloc_fulfilled_heap;
    let body_v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::Str, resp_op, 16),
        Type::Str,
        None,
    );
    ctx.emit_rc_inc(Operand::Value(body_v));
    let p_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(promise_alloc_fulfilled_heap, vec![Operand::Value(body_v)]),
        Type::Promise,
        None,
    );
    Some(Operand::Value(p_v))
}

/// `<bunfile>.text() / .exists()` — receiver expression-tree shape
/// must be `Bun.file(...)` so we don't intercept user .text()/.exists()
/// methods on other objects. `.text()` reads via `fs_read_file_sync`,
/// `.exists()` via `fs_exists_sync` then wraps in a fulfilled Promise.
/// MVP "synchronous-then-resolve"; real I/O suspension lands with the
/// state-machine async/await trunk.
fn try_lower_bunfile_method(
    ctx: &mut LowerCtx<'_>,
    file_id: ExprId,
    m_name: &str,
) -> Option<Operand> {
    let is_bun_file = matches!(
        ctx.ast.get_expr(file_id),
        Expr::Call { callee: f_callee, .. }
            if matches!(
                ctx.ast.get_expr(*f_callee),
                Expr::Member { obj: ns_id, name: m }
                    if m == "file"
                        && matches!(
                            ctx.ast.get_expr(*ns_id),
                            Expr::Ident(ns) if ns == "Bun"
                        )
            )
    );
    if !is_bun_file {
        return None;
    }
    let path_op = ctx.lower_expr(file_id);
    let cur_block = ctx.cur_block;
    let promise_alloc_fulfilled_heap = ctx.intrinsics.promise_alloc_fulfilled_heap;
    let promise_alloc_fulfilled = ctx.intrinsics.promise_alloc_fulfilled;
    if m_name == "text" {
        let fs_read_file_sync = ctx.intrinsics.fs_read_file_sync;
        let str_v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(fs_read_file_sync, vec![path_op]),
            Type::Str,
            None,
        );
        let p_v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(promise_alloc_fulfilled_heap, vec![Operand::Value(str_v)]),
            Type::Promise,
            None,
        );
        return Some(Operand::Value(p_v));
    }
    // m_name == "exists": Bool result → coerce to i64 → primitive Promise.
    let fs_exists_sync = ctx.intrinsics.fs_exists_sync;
    let bool_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fs_exists_sync, vec![path_op]),
        Type::Bool,
        None,
    );
    let arg_i64 = ctx.coerce_bool_to_i64(Operand::Value(bool_v));
    let cur_block = ctx.cur_block;
    let p_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(promise_alloc_fulfilled, vec![arg_i64]),
        Type::Promise,
        None,
    );
    Some(Operand::Value(p_v))
}

/// `__torajs_weakref_create(target)` — observation only. The target is
/// NOT consumed: the registry observes the ptr without keeping it
/// alive, and the binding the user wrote (`new WeakRef(b)` with `b`
/// aliased) keeps normal drop semantics. The generic Call path would
/// mark the arg consumed and skip the surrounding scope's drop walk,
/// so we intercept here.
fn try_lower_weakref_create(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let target_op = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    let weakref_create = ctx.intrinsics.weakref_create;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(weakref_create, vec![target_op]),
        Type::WeakRef,
        None,
    );
    Some(Operand::Value(v))
}
