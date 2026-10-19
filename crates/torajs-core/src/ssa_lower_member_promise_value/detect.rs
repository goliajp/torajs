//! Is an awaited operand a built-in promise? By the binding's slot, by
//! the shape of the call that made it, or by the checker's verdict.

use crate::ast::{Expr, ExprId};
use crate::check::{self as check_mod};
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

pub(crate) fn detect_obj_is_builtin_promise(ctx: &LowerCtx<'_>, obj: ExprId) -> bool {
    match ctx.ast.get_expr(obj) {
        Expr::Ident(n) => ctx
            .locals
            .get(n)
            .map(|info| matches!(info.ty, Type::Promise))
            .unwrap_or(false),
        // Call shape probe first; fall back to the checker's verdict —
        // a multi-owner method call (`it.next()` with sync AND async
        // generator classes in scope) stays a Member call for
        // sibling-class dispatch, so no callee shape matches, but the
        // checker typed the call Promise<T> all the same (RFC 20260713
        // blade 4: async-generator next()).
        Expr::Call { callee, .. } => {
            detect_promise_returning_call(ctx, *callee)
                || matches!(ctx.expr_types.get(&obj), Some(check_mod::Type::Promise(_)))
        }
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
    // A `.then` result is a built-in Promise only when the receiver
    // itself is one. Through an `any` receiver the call is served by
    // the runtime's any-lane bridge, whose result cell stamps
    // REPR_ANY and stores a BOXED settled value — reading it with the
    // typed `get_value` hands back the box's raw bits (an i64 10 read
    // as -562949953421302). Mirrors the lowering side's
    // `src_is_builtin_promise` gate, which already declines an `any`
    // source; before this the two predicates disagreed and `await
    // anyPromise.then(cb)` took the typed read.
    let then_chain = matches!(
        ctx.ast.get_expr(callee),
        Expr::Member { obj: src_id, name: m_name }
            if (m_name == "then" || m_name == "catch" || m_name == "finally")
                && detect_obj_is_builtin_promise(ctx, *src_id)
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
