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

use super::{Ast, Expr, ExprId, Stmt, default_init_for_type};

pub fn desugar_async(ast: &mut Ast) {
    rewrite_async_fn_value_exprs(ast);
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
        let (name, type_params, params, return_type, body, span) = match &ast.stmts[idx] {
            Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                body,
                span,
                ..
            } => (
                name.clone(),
                type_params.clone(),
                params.clone(),
                return_type.clone(),
                body.clone(),
                *span,
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
        let (wrapped_body, promise_ty) = build_async_body(ast, body, &declared_ty);

        ast.stmts[idx] = Stmt::FnDecl {
            name,
            type_params,
            params,
            return_type: Some(promise_ty),
            body: wrapped_body,
            is_generator: false,
            // in-place rewrite of the user's declaration -- toString
            // answers the source text they wrote (B1b)
            span,
        };
    }
}

/// RFC 20260713 blade 3 — async function-VALUE expressions (`async
/// () => ...` / `async x => ...` / `async function(){}`). Each marked
/// `Expr::ArrowFn` gets the same body transform as an async FnDecl,
/// applied in place while the body is still inside the expression —
/// `lift_arrow_fns` runs later, so the lifted `__closure_N` is an
/// ordinary Promise-returning closure and captures ride the normal
/// `__env` channel (no async_fns registration, no pass reordering).
fn rewrite_async_fn_value_exprs(ast: &mut Ast) {
    if ast.async_fn_value_exprs.is_empty() {
        return;
    }
    // Deterministic order: the rewrites append fresh exprs to the
    // arena, so iterating the HashSet directly would make the arena
    // layout (and the compiled artifact) run-to-run unstable.
    let mut ids: Vec<ExprId> = ast.async_fn_value_exprs.iter().copied().collect();
    ids.sort_by_key(|e| e.0);
    for eid in ids {
        let i = eid.0 as usize;
        let arrow = std::mem::replace(&mut ast.exprs[i], Expr::Null);
        let Expr::ArrowFn {
            params,
            return_type,
            body,
        } = arrow
        else {
            panic!("async_fn_value_exprs marker on a non-ArrowFn ExprId {eid:?} (parser contract)");
        };
        let declared_ty = return_type.unwrap_or_else(|| "any".into());
        let (wrapped_body, promise_ty) = build_async_body(ast, body, &declared_ty);
        ast.exprs[i] = Expr::ArrowFn {
            params,
            return_type: Some(promise_ty),
            body: wrapped_body,
        };
    }
}

/// Shared async body transform (FnDecl and ArrowFn shapes): rewrite
/// every `return e` to `return Promise.resolve(e)`, add the
/// tail-safety default return, and wrap the whole body in `try {...}
/// catch (__async_err: any) { return Promise.reject(__async_err) }`.
/// Returns the wrapped body plus the `Promise<T>` annotation for the
/// function's return slot.
///
/// P10.3-A2 — accept both annotation forms:
///   `async function f(): number { ... }`          → inner_ty = "number"
///   `async function f(): Promise<number> { ... }` → inner_ty = "number"
/// Pre-P10.3-A2 the desugar unconditionally prepended `Promise<...>`,
/// double-wrapping the idiomatic TS form into `Promise<Promise<T>>`.
///
/// P10.5-A1 — the try/catch wrap is ES spec §27.7.3.6
/// AsyncFunctionBody: an uncaught throw becomes a rejected Promise
/// instead of propagating synchronously to the caller. P10.5-A2 —
/// `catch_type` is `any` (spec-correct; `throw <expr>` may differ from
/// inner T), accepted by `Promise.reject` at `check/promise_static.rs`.
/// Shared with the class-method path
/// (`desugar_classes_emit::maybe_rewrite_async_method_body`) — the two
/// used to hold the same transform twice, and only this copy was taught
/// the P10.7 default-`any`, so an async METHOD without a return
/// annotation kept panicking long after the top-level requirement was
/// lifted.
pub(super) fn build_async_body(
    ast: &mut Ast,
    body: Vec<Stmt>,
    declared_ty: &str,
) -> (Vec<Stmt>, String) {
    let inner_ty = if let Some(rest) = declared_ty.strip_prefix("Promise<")
        && let Some(inner) = rest.strip_suffix('>')
    {
        inner.to_string()
    } else {
        declared_ty.to_string()
    };
    let promise_ty = format!("Promise<{inner_ty}>");

    // T-15.h: rewrite each `return e;` to `return Promise.resolve(e);`.
    // No shared `__async_p` state — every return constructs a fresh
    // fulfilled built-in Promise.
    let mut new_body: Vec<Stmt> = Vec::with_capacity(body.len() + 1);
    for s in body {
        let mut s = s;
        rewrite_returns_for_async(ast, &mut s, &inner_ty);
        new_body.push(s);
    }
    // Tail safety: if control flow falls off the end, return
    // `Promise.resolve(<default T>)`.
    if !body_ends_in_return(&new_body) {
        let default_id = ast.add_expr(default_init_for_type(&inner_ty));
        let call = build_async_resolve(ast, default_id, &inner_ty);
        new_body.push(Stmt::Return(Some(call)));
    }

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
    (wrapped_body, promise_ty)
}

/// True if the (possibly-empty) body's last reachable statement is a
/// `Stmt::Return`. Used by `desugar_async` to skip emitting the tail-
/// safety `return __async_p;` when the body already ends in one (a
/// second access of `__async_p` would trip tr's move tracker even on
/// the unreachable path).
fn body_ends_in_return(body: &[Stmt]) -> bool {
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

/// Build the `Promise.resolve(value)` an async return hands back, in the
/// shape the declared inner type asks for.
///
/// P10.7 — a default-Any async fn wraps the value in `Expr::As { …,
/// ty_ann: "any" }` so the call typechecks as `Promise<Any>` (matching the
/// declared fn return) instead of inferring `Promise<concrete>` from the
/// raw value's static type. Explicit-T async fns pass the value straight
/// through.
///
/// The tail-safety return needs the very same wrap. "Settled with
/// undefined" has two representations in tr — a `Promise<Undefined>`
/// carrying no value, and an Any box holding ANY_UNDEF — and `await`
/// decodes against the *declared* inner type. So an Any-returning function
/// that fell off its end used to hand back the first while its awaiter read
/// the second, answering `[unknown-any-tag]`.
fn build_async_resolve(ast: &mut Ast, raw_value: ExprId, inner_ty: &str) -> ExprId {
    let value = if inner_ty == "any" {
        ast.add_expr(Expr::As {
            expr: raw_value,
            ty_ann: "any".into(),
        })
    } else {
        raw_value
    };
    let promise_ident = ast.add_expr(Expr::Ident("Promise".into()));
    let resolve_member = ast.add_expr(Expr::Member {
        obj: promise_ident,
        name: "resolve".into(),
    });
    ast.add_expr(Expr::Call {
        callee: resolve_member,
        args: vec![value],
    })
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
fn rewrite_returns_for_async(ast: &mut Ast, s: &mut Stmt, inner_ty: &str) {
    match s {
        Stmt::Return(maybe) => {
            let raw_value = match maybe {
                Some(eid) => *eid,
                None => {
                    let default = default_init_for_type(inner_ty);
                    ast.add_expr(default)
                }
            };
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
