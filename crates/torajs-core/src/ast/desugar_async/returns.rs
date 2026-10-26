//! The return rewrite of an async body (`desugar_async`), and the test
//! for whether a body already ends in a return.

use super::super::{Ast, Expr, Stmt, default_init_for_type};
use super::resolve::build_async_resolve;

/// True if the (possibly-empty) body's last reachable statement is a
/// `Stmt::Return`. Used by `desugar_async` to skip emitting the tail-
/// safety `return __async_p;` when the body already ends in one (a
/// second access of `__async_p` would trip tr's move tracker even on
/// the unreachable path).
pub(super) fn body_ends_in_return(body: &[Stmt]) -> bool {
    match body.last() {
        Some(Stmt::Return(_)) => true,
        // A trailing `throw` is a terminal completion too — control
        // never falls through, so the async tail-safety return is
        // both unreachable AND (for struct inner types like the
        // async-generator `__step_*`) wrongly typed: the
        // default_init_for_type catch-all answers Number(0.0), which
        // fails check as Promise(Number) vs Promise(Struct).
        Some(Stmt::Throw(_)) => true,
        Some(Stmt::Multi(stmts)) | Some(Stmt::Block(stmts)) => body_ends_in_return(stmts),
        _ => false,
    }
}

/// T-15.h (v0.5.0) — recursively rewrite `Stmt::Return(Some(e))` /
/// `Stmt::Return(None)` inside `s` into `Stmt::Return(Promise.resolve(e))`.
///
/// Pre-T-15.h MVP wrapped each return in a user-class `__async_p`
/// shared across the function body (`__async_p.do_resolve(e); return
/// __async_p;`). With the built-in Promise<T> from T-15, every return
/// just constructs a fresh fulfilled Promise — no shared state, no
/// move-tracker complications, no need for the user to declare
/// `class Promise<T>` themselves.
pub(super) fn rewrite_returns_for_async(ast: &mut Ast, s: &mut Stmt, inner_ty: &str) {
    match s {
        Stmt::Return(maybe) => {
            let raw_value = match maybe {
                Some(eid) => *eid,
                None => {
                    let default = default_init_for_type(inner_ty);
                    ast.add_expr(default)
                }
            };
            // A returned object LITERAL binds to a `const` declared as
            // the inner type before it reaches `Promise.resolve`. The
            // rewrite turns the literal into a call argument, and the
            // one site that pins a declared struct layout onto a
            // literal — the ret-lowering of a fn declared to return
            // that struct — no longer sees it (the fn returns a
            // Promise now). Un-hinted, the literal took whichever
            // same-named layout registered first: an async generator's
            // `{value: <f64>, done}` boxed its value into a sync
            // generator's `{value: any, done}` step, and every reader
            // parsed the declared type and loaded the box bits as f64
            // (`a 128 false` for a stored 100; K5, rotation 641). Not
            // generator-specific — `async function f(): Promise<{value:
            // number; done: boolean}>` read the same way. The typed
            // `const` is the let-decl hint every other literal takes.
            if inner_ty != "any" && matches!(ast.get_expr(raw_value), Expr::ObjectLit { .. }) {
                let name = format!("__aret_{}", raw_value.0);
                let decl = Stmt::LetDecl {
                    mutable: false,
                    name: name.clone(),
                    type_ann: Some(inner_ty.to_string()),
                    init: raw_value,
                    is_var: false,
                };
                let read = ast.add_expr(Expr::Ident(name));
                let call = build_async_resolve(ast, read, inner_ty);
                *s = Stmt::Block(vec![decl, Stmt::Return(Some(call))]);
                return;
            }
            let call = build_async_resolve(ast, raw_value, inner_ty);
            *s = Stmt::Return(Some(call));
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_returns_for_async(ast, then_branch, inner_ty);
            if let Some(eb) = else_branch.as_deref_mut() {
                rewrite_returns_for_async(ast, eb, inner_ty);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_returns_for_async(ast, body, inner_ty);
        }
        Stmt::Labeled { body, .. } => {
            rewrite_returns_for_async(ast, body, inner_ty);
        }
        // probe aw7-forof — a `return` inside an async body's for-of
        // kept its bare value (this match simply never had the arm):
        // annotated fns failed check, unannotated ones leaked the
        // bare value out of the Promise lane.
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            rewrite_returns_for_async(ast, body, inner_ty);
        }
        Stmt::For { body, init, .. } => {
            if let Some(i) = init.as_deref_mut() {
                rewrite_returns_for_async(ast, i, inner_ty);
            }
            rewrite_returns_for_async(ast, body, inner_ty);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                rewrite_returns_for_async(ast, s, inner_ty);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for s in &mut c.body {
                    rewrite_returns_for_async(ast, s, inner_ty);
                }
            }
            if let Some(d) = default {
                for s in d {
                    rewrite_returns_for_async(ast, s, inner_ty);
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                rewrite_returns_for_async(ast, s, inner_ty);
            }
            for s in catch_body {
                rewrite_returns_for_async(ast, s, inner_ty);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    rewrite_returns_for_async(ast, s, inner_ty);
                }
            }
        }
        _ => {}
    }
}
