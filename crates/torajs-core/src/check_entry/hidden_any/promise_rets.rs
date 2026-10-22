//! Functions declared to return `Promise<T>` that return an untyped
//! promise.
//!
//! `function src(x: any): Promise<number> { return Promise.resolve(x) }`
//! returns a promise settled with `x` as it is (§27.2.4.7), whatever the
//! annotation says, and §10.2.1.12 returns it unchanged. Typed by the
//! annotation, a call's `await` and handlers read a string through the
//! number lane. Code after an `await` has no `any` version to fall back
//! to at run time, so the function returns `Promise<any>`, the way a
//! scalar return that hands back an `any` value is retyped `any`
//! (`super::retype_returns_any`). An async function declared
//! `Promise<T>` whose body returns an `any` value is the same case:
//! §27.7.5.3 resolves its promise with the value as it is, unless the
//! static sniff proved every return is `T`. A
//! `Promise.reject(r)` types `Promise<any>` as the `never` a rejected
//! promise fulfils with, and leaves the declared return as it is.

use std::collections::{HashMap, HashSet};

use crate::ast::desugar_with::walk::stmt_children_ref;
use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ast_desugar_builtin_new::promise::context::promise_inner;
use crate::check::Type;
use crate::check_type_of_call_promise_then::is_reject_mint;

/// Add to `found` each function of `ast` declared `Promise<T>` (with a
/// `T` other than `any`) that `types` shows returning a promise of
/// `any`, or, for an async function, an `any` value; true when one was
/// new.
pub(super) fn extend(
    ast: &Ast,
    types: &HashMap<ExprId, Type>,
    found: &mut HashSet<String>,
) -> bool {
    let before = found.len();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        let Stmt::FnDecl {
            name,
            return_type: Some(rt),
            body,
            ..
        } = s
        else {
            return;
        };
        if found.contains(name)
            || driven(ast, name)
            || ast
                .async_proved_returns
                .get(name)
                .is_some_and(|p| p.any_args())
            || promise_inner(rt).is_none_or(|i| !typed_spelling(&i))
        {
            return;
        }
        let untyped = (own_returns(body).into_iter())
            .filter(|e| !is_reject_mint(ast, *e))
            .any(|e| settles_any(ast, types, e));
        if untyped {
            found.insert(name.clone());
        }
    });
    found.len() > before
}

/// Does a `Promise<T>` spelled with `inner` read its value on a typed
/// lane? `any`, `unknown`, a union and a nullable take every value on
/// the any lane already, so a promise settled with a value of any kind
/// is read as it is.
fn typed_spelling(inner: &str) -> bool {
    !matches!(inner, "any" | "unknown")
        && !inner.contains('|')
        && !inner.contains("__nullable(")
        && !inner.contains("__union(")
}

/// An async function whose body a generator runs, an async generator,
/// or a lifted async function value: what its declaration returns is
/// the driver's promise or the generator object, not a settle of a
/// value its body computed.
fn driven(ast: &Ast, name: &str) -> bool {
    ast.async_sm_fns.contains(name)
        || ast.async_generator_fns.contains(name)
        || ast.fn_async_value_fns.contains(name)
}

/// Does the returned `e` hand back a promise settled with a value of
/// any kind? A promise the program built says so by its type. The
/// async desugar's own settle of a `return v` (`Promise.resolve(v)`)
/// settles `v` as it is, so it says so when `v` is `any`.
fn settles_any(ast: &Ast, types: &HashMap<ExprId, Type>, e: ExprId) -> bool {
    if ast.synth_promise_static_calls.contains(&e) {
        return matches!(ast.get_expr(e), Expr::Call { args, .. }
            if args.len() == 1 && types.get(&args[0]) == Some(&Type::Any));
    }
    matches!(types.get(&e), Some(Type::Promise(i)) if **i == Type::Any)
}

/// The values a function body returns; a nested function's are its own.
fn own_returns(body: &[Stmt]) -> Vec<ExprId> {
    let mut out = Vec::new();
    let mut stack: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = stack.pop() {
        match s {
            Stmt::Return(Some(e)) => out.push(*e),
            Stmt::FnDecl { .. } => {}
            s => stack.extend(stmt_children_ref(s)),
        }
    }
    out
}

/// Annotate `Promise<any>` the return of every function in `fns`.
pub(super) fn retype(stmts: &mut [Stmt], fns: &HashSet<String>) {
    let mut stack: Vec<&mut Stmt> = stmts.iter_mut().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl {
            name, return_type, ..
        } = s
            && fns.contains(name)
        {
            *return_type = Some("Promise<any>".to_string());
        }
        crate::ast_closure_param_tag_retag::push_child_stmts_mut(s, &mut stack);
    }
}
