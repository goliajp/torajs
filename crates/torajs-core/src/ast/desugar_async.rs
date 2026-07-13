//! `async function f(...): T { ... }` body rewrite pass.
//!
//! Extracted from `ast.rs::desugar_async` (2026-06-03, god-file
//! decomp — P10.5-A1 prerequisite per `file-size.md` "先拆再加"
//! rule). The pass walks every `Stmt::FnDecl` whose name is in
//! `ast.async_fns`, rewrites each `return e;` inside its body to
//! `return Promise.resolve(e);`, appends a tail-safety return if
//! control flow falls through, and collapses the declared return
//! type to `Promise<inner_ty>`.
//!
//! Runs between `desugar_generators` and `desugar_classes` so the
//! generated `Promise.resolve(...)` Member-access expressions are
//! still in pre-desugar shape — `desugar_classes` rewrites
//! `Promise.X` static-member calls to `__sm_Promise__X` afterwards,
//! tying everything to the runtime's static dispatch table.
//!
//! Helpers `body_ends_in_return` / `rewrite_returns_for_async` are
//! re-exported from the parent `ast` module so sibling sub-modules
//! (`ast::desugar_classes_emit`) keep their existing `use super::*`
//! access path unchanged.

use super::{Ast, Expr, Stmt, default_init_for_type};

pub fn desugar_async(ast: &mut Ast) {
    if ast.async_fns.is_empty() {
        return;
    }
    // Find every async FnDecl by index so we can mutate ast.stmts in
    // place. We only touch stmts; field shapes stay otherwise unchanged.
    let async_indices: Vec<usize> = ast
        .stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::FnDecl { name, .. } if ast.async_fns.contains(name) => Some(i),
            _ => None,
        })
        .collect();

    for idx in async_indices {
        // Snapshot the FnDecl pieces so we can rebuild it in place.
        let (name, type_params, params, return_type, body) = match &ast.stmts[idx] {
            Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                body,
                ..
            } => (
                name.clone(),
                type_params.clone(),
                params.clone(),
                return_type.clone(),
                body.clone(),
            ),
            _ => unreachable!(),
        };
        // P10.7 — Default-Any async fn. When the user omits the
        // return-type annotation (`async function foo() {...}`),
        // infer `Promise<any>` so the body's `return e` rewrites
        // produce `Promise.resolve(e as any)` (the `As`-wrap
        // happens inside `rewrite_returns_for_async` when
        // inner_ty == "any"). `Promise.resolve` accepts Any at
        // `check/promise_static.rs`; `.then` / `.catch` accept
        // `Promise<Any>` at `check.rs:5008` / `:5054`.
        let declared_ty = return_type.unwrap_or_else(|| "any".into());
        // P10.3-A2 — accept both annotation forms:
        //   `async function f(): number { ... }`        → inner_ty = "number"
        //   `async function f(): Promise<number> { ... }` → inner_ty = "number"
        // Pre-P10.3-A2 the desugar unconditionally prepended `Promise<...>`,
        // double-wrapping the idiomatic TS form into `Promise<Promise<T>>` and
        // surfacing as "return type mismatch: function expects
        // Promise(Promise(Number)), got Promise(Number)" at first `return e`.
        // Strip a leading `Promise<...>` wrapper (if any) to recover the
        // inner T; the body's `return e` rewrites still produce
        // `Promise.resolve(e)` of type Promise<T>, matching the resolved
        // promise_ty below.
        let inner_ty = if let Some(rest) = declared_ty.strip_prefix("Promise<")
            && let Some(inner) = rest.strip_suffix('>')
        {
            inner.to_string()
        } else {
            declared_ty.clone()
        };
        let promise_ty = format!("Promise<{inner_ty}>");

        // T-15.h: rewrite each `return e;` to `return Promise.resolve(e);`.
        // No shared `__async_p` state — every return constructs a
        // fresh fulfilled built-in Promise. Cleaner than the v0.4.x
        // user-class MVP and removes the multi-return move-tracker
        // workaround.
        let mut new_body: Vec<Stmt> = Vec::with_capacity(body.len() + 1);
        for s in body {
            let mut s = s;
            rewrite_returns_for_async(ast, &mut s, &inner_ty);
            new_body.push(s);
        }
        // Tail safety: if control flow falls off the end, return
        // `Promise.resolve(<default T>)`.
        if !body_ends_in_return(&new_body) {
            let default_init = default_init_for_type(&inner_ty);
            let default_id = ast.add_expr(default_init);
            let promise_ident = ast.add_expr(Expr::Ident("Promise".into()));
            let resolve_member = ast.add_expr(Expr::Member {
                obj: promise_ident,
                name: "resolve".into(),
            });
            let call = ast.add_expr(Expr::Call {
                callee: resolve_member,
                args: vec![default_id],
            });
            new_body.push(Stmt::Return(Some(call)));
        }

        // P10.5-A1 — wrap the rewritten body in `try { ... } catch
        // (__async_err: any) { return Promise.reject(__async_err); }`
        // per ES spec §27.7.3.6 AsyncFunctionBody. Without this an
        // uncaught throw inside an async fn would propagate
        // synchronously to the caller instead of becoming a rejected
        // Promise, short-circuiting the sync continuation and
        // skipping unhandled-rejection handling entirely (the wider
        // P10.5 handler hook builds on this).
        //
        // P10.5-A2 — `catch_type` is `any` (spec-correct), upgraded
        // from the A1-era narrow MVP that pinned it to the fn's
        // declared inner T. The shadow type now matches what the
        // user actually wrote in `throw <expr>` (which may differ
        // from inner T — e.g. `async fn f(): Promise<number> { throw
        // "err"; }`); `Promise.reject` accepts `any` at
        // `check/promise_static.rs`, dispatching to the existing
        // heap reject path at SSA (boxed-any pointer is i64-sized).
        let err_ident_for_arg = ast.add_expr(Expr::Ident("__async_err".into()));
        let promise_ident_rj = ast.add_expr(Expr::Ident("Promise".into()));
        let reject_member = ast.add_expr(Expr::Member {
            obj: promise_ident_rj,
            name: "reject".into(),
        });
        let reject_call = ast.add_expr(Expr::Call {
            callee: reject_member,
            args: vec![err_ident_for_arg],
        });
        let catch_body = vec![Stmt::Return(Some(reject_call))];
        let wrapped_body = vec![Stmt::Try {
            body: new_body,
            had_catch: true,
            catch_param: Some("__async_err".into()),
            catch_type: Some("any".into()),
            catch_body,
            finally_body: None,
        }];

        ast.stmts[idx] = Stmt::FnDecl {
            name,
            type_params,
            params,
            return_type: Some(promise_ty),
            body: wrapped_body,
            is_generator: false,
        };
    }
}

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
            // P10.7 — Default-Any async fn: wrap the return value in
            // `Expr::As { …, ty_ann: "any" }` so `Promise.resolve(...)`
            // typechecks as `Promise<Any>` (matching the declared fn
            // return) instead of inferring `Promise<concrete>` from
            // the raw value's static type. Explicit-T async fns
            // (`inner_ty != "any"`) keep the direct call.
            let value = if inner_ty == "any" {
                ast.add_expr(Expr::As {
                    expr: raw_value,
                    ty_ann: "any".into(),
                })
            } else {
                raw_value
            };
            // Build `Promise.resolve(value)` AST.
            let promise_ident = ast.add_expr(Expr::Ident("Promise".into()));
            let resolve_member = ast.add_expr(Expr::Member {
                obj: promise_ident,
                name: "resolve".into(),
            });
            let call = ast.add_expr(Expr::Call {
                callee: resolve_member,
                args: vec![value],
            });
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
