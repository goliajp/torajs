//! T-15.g.2 (v0.5.0) — `await p` (= parser-desugared `p.value`) on
//! a built-in `Type::Promise(T)` plus P10.4 fast-path identity for
//! `await <primitive>`. Pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Member` god-arm
//! as chunk-62 of the decomp (chunks 1-61 = ... + T-27.c
//! `f.length`/`f.name` dual-path).
//!
//! The Promise path lowers to a runtime
//! `__torajs_promise_get_value(p)` call which reads the resolved
//! i64 value slot, with microtask drain before and value-cast +
//! rc_inc after. Static-only dispatch: only fire when we can
//! determine `obj`'s type IS `Type::Promise` without lowering
//! (eager lowering would double-side-effect the obj subexpression
//! on fall-through).
//!
//! **Built-in promise detection** — recognise the source shape from
//! the AST so the fast path stays narrow:
//! - Ident bound to `Type::Promise` in `LowerCtx::locals`
//! - Call result: `Promise.{resolve|reject|all|race|any|allSettled}`,
//!   `fs_promises.{readFile|writeFile|appendFile|unlink|mkdir|exists|
//!   readdir}`, `Bun.file(...).{text|exists}()`, `<promise>.{then|
//!   catch|finally}` chain result, user fn returning `Type::Promise`
//!   (via `signatures`), `fetch(url)`, `<response>.text()`
//! - Any other receiver typed `check::Type::Promise(_)` via
//!   `expr_types` (covers `await ps[i]` indexing, tuple destructure,
//!   etc.)
//!
//! **P10.4 identity fast-path** — `await <primitive>` collapses to
//! the operand itself per ES spec (`Promise.resolve(e)` for
//! non-thenable yields e). Whitelist of `check::Type` Number / String
//! / Boolean / Array(_) / BigInt: lower obj directly, no promise
//! alloc, no microtask drain. `Type::Object("...")` and
//! `Type::Struct` are NOT in the whitelist because they CAN carry a
//! real user-defined `.value` field — those fall through to the
//! regular Member path. Pair this arm with the matching check.rs
//! arm so typecheck + lowering agree.
//!
//! **Value-cast switch** — `promise_get_value` returns i64
//! regardless of T; SSA value-table needs the proper shape:
//! - heap T (`Str / Substr / Obj / Arr / Closure / RegExp / Date /
//!   Symbol / Promise / Any / Ptr`): `IntToPtr` + `emit_rc_inc`
//!   (T-19.j: Promise owns ONE ref on inner value; when temp
//!   Promise drops, value_drop_heap dec's that ref — without
//!   `rc_inc` the value frees BEFORE the user reads it, UAF that
//!   accidentally worked for n≤32 via pool block reuse but
//!   corrupted at n>32 where libc free is real).
//! - `Bool`: `TruncI64ToBool`.
//! - `F64`: `BitCastI64ToF64` (②.6b — resolve/cb-thunk write f64
//!   bits, decode symmetrically).
//! - Other / None: raw i64 (covers Promise<Number> and legacy
//!   `lower(...)` entry point with empty check map).
//!
//! P10.4-A2: post-call `emit_throw_check(None)` propagates
//! catchable throw slot rejections into the user's `try/catch`.
//!
//! Returns `Some(op)` on hit; `None` on miss (Member name not
//! `value`, or obj is neither built-in Promise nor primitive
//! whitelist).

use crate::ast::{Expr, ExprId};
use crate::check::{self as check_mod};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_parse_type::parse_type;

pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, obj: ExprId, name: &str) -> Option<Operand> {
    if name != "value" {
        return None;
    }
    let obj_is_builtin_promise = detect_obj_is_builtin_promise(ctx, obj);
    if !obj_is_builtin_promise {
        return try_p10_4_primitive_identity(ctx, obj);
    }
    Some(lower_promise_get_value(ctx, obj))
}

fn detect_obj_is_builtin_promise(ctx: &LowerCtx<'_>, obj: ExprId) -> bool {
    match ctx.ast.get_expr(obj) {
        Expr::Ident(n) => ctx
            .locals
            .get(n)
            .map(|info| matches!(info.ty, Type::Promise))
            .unwrap_or(false),
        Expr::Call { callee, .. } => detect_promise_returning_call(ctx, *callee),
        _ => matches!(ctx.expr_types.get(&obj), Some(check_mod::Type::Promise(_))),
    }
}

fn detect_promise_returning_call(ctx: &LowerCtx<'_>, callee: ExprId) -> bool {
    let static_ctor = matches!(
        ctx.ast.get_expr(callee),
        Expr::Member { obj: ns_id, name: m_name }
            if matches!(
                m_name.as_str(),
                "resolve" | "reject" | "all" | "race" | "any" | "allSettled"
            ) && matches!(
                ctx.ast.get_expr(*ns_id),
                Expr::Ident(ns) if ns == "Promise"
            )
    );
    let fs_async = matches!(
        ctx.ast.get_expr(callee),
        Expr::Member { obj: ns_id, name: m_name }
            if matches!(
                m_name.as_str(),
                "readFile" | "writeFile" | "appendFile"
                    | "unlink" | "mkdir" | "exists" | "readdir"
            ) && matches!(
                ctx.ast.get_expr(*ns_id),
                Expr::Ident(ns) if ns == "fs_promises"
            )
    );
    let bun_file_text = matches!(
        ctx.ast.get_expr(callee),
        Expr::Member { obj: file_id, name: m_name }
            if (m_name == "text" || m_name == "exists")
                && matches!(
                    ctx.ast.get_expr(*file_id),
                    Expr::Call { callee: f_callee, .. }
                        if matches!(
                            ctx.ast.get_expr(*f_callee),
                            Expr::Member { obj: ns_id, name: fm }
                                if fm == "file"
                                    && matches!(
                                        ctx.ast.get_expr(*ns_id),
                                        Expr::Ident(ns) if ns == "Bun"
                                    )
                        )
                )
    );
    let then_chain = matches!(
        ctx.ast.get_expr(callee),
        Expr::Member { name: m_name, .. }
            if m_name == "then" || m_name == "catch" || m_name == "finally"
    );
    let fn_returns_promise = if let Expr::Ident(fn_name) = ctx.ast.get_expr(callee) {
        ctx.fn_table
            .get(fn_name)
            .copied()
            .and_then(|fid| ctx.signatures.get(&fid).copied())
            .map(|ty| matches!(ty, Type::Promise))
            .unwrap_or(false)
    } else {
        false
    };
    let fetch_call = matches!(
        ctx.ast.get_expr(callee),
        Expr::Ident(n) if n == "fetch"
    );
    let response_text = matches!(
        ctx.ast.get_expr(callee),
        Expr::Member { obj: resp_id, name: m_name }
            if m_name == "text"
                && matches!(
                    ctx.expr_types.get(resp_id),
                    Some(crate::check::Type::Object("Response"))
                )
    );
    static_ctor
        || then_chain
        || fn_returns_promise
        || fs_async
        || bun_file_text
        || fetch_call
        || response_text
}

fn try_p10_4_primitive_identity(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Option<Operand> {
    if !matches!(
        ctx.expr_types.get(&obj),
        Some(check_mod::Type::Number)
            | Some(check_mod::Type::String)
            | Some(check_mod::Type::Boolean)
            | Some(check_mod::Type::Array(_))
            | Some(check_mod::Type::BigInt)
    ) {
        return None;
    }
    Some(ctx.lower_expr(obj))
}

fn lower_promise_get_value(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Operand {
    let obj_op = ctx.lower_expr(obj);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.microtask_drain, vec![]),
    );
    let inner_ssa_ty = recover_inner_ssa_ty(ctx, obj);
    let inner_ssa_ty = ctx.widen_promise_inner_ty(inner_ssa_ty, obj);
    let cur_block = ctx.cur_block;
    let raw_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.promise_get_value, vec![obj_op.clone()]),
        Type::I64,
        None,
    );
    ctx.emit_throw_check(None);
    let v = cast_promise_value(ctx, raw_v, inner_ssa_ty);
    let is_borrow = matches!(
        ctx.ast.get_expr(obj),
        Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
    );
    if !is_borrow {
        ctx.emit_drop_value(obj_op, Type::Promise);
    }
    Operand::Value(v)
}

fn recover_inner_ssa_ty(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Option<Type> {
    let ann = if let Some(check_mod::Type::Promise(inner)) = ctx.expr_types.get(&obj) {
        check_mod::type_to_ann(inner)
    } else {
        return None;
    };
    Some(parse_type(
        Some(&ann),
        ctx.aliases,
        ctx.arr_layouts,
        ctx.fn_sigs,
        ctx.generic_struct_decls,
        ctx.struct_layouts,
        ctx.inst_memo,
    ))
}

fn cast_promise_value(
    ctx: &mut LowerCtx<'_>,
    raw_v: crate::ssa::ValueId,
    inner_ssa_ty: Option<Type>,
) -> crate::ssa::ValueId {
    let cur_block = ctx.cur_block;
    match inner_ssa_ty {
        Some(t)
            if matches!(
                t,
                Type::Str
                    | Type::Substr
                    | Type::Obj(_)
                    | Type::Arr(_)
                    | Type::Closure(_)
                    | Type::RegExp
                    | Type::Date
                    | Type::Symbol
                    | Type::Promise
                    | Type::Any
                    | Type::Ptr
            ) =>
        {
            let ptr = ctx.f.append_inst(
                cur_block,
                InstKind::IntToPtr(Operand::Value(raw_v)),
                t,
                None,
            );
            ctx.emit_rc_inc(Operand::Value(ptr));
            ptr
        }
        Some(Type::Bool) => ctx.f.append_inst(
            cur_block,
            InstKind::TruncI64ToBool(Operand::Value(raw_v)),
            Type::Bool,
            None,
        ),
        Some(Type::F64) => ctx.f.append_inst(
            cur_block,
            InstKind::BitCastI64ToF64(Operand::Value(raw_v)),
            Type::F64,
            None,
        ),
        _ => raw_v,
    }
}
