//! `new Promise<T>` whose executor resolves with an `any` value.
//!
//! `new Promise<number>((r) => r(x))` with `x: any` holding `"s"`
//! settles the string: §27.2.1.3.2 resolves with the value as it is, and
//! the `resolve` parameter's annotation converts nothing. The desugar
//! made the expression a call of the typed helper
//! `__promise_from_executor__number`, so the promise typed
//! `Promise<number>` and `await` read the string through the number
//! lane. Code after an `await` has no `any` version to fall back to at
//! run time, so such a call is read as `Promise<any>`: the same
//! retyping a function returning a promise of `any` gets
//! (`super::promise_rets`).

use std::collections::{HashMap, HashSet};

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ast_desugar_builtin_new::promise::context::helper_call;
use crate::check::Type;

/// Add to `found` each typed executor-helper call whose executor may
/// settle a value of any kind; true when one was new.
pub(super) fn extend(
    ast: &Ast,
    types: &HashMap<ExprId, Type>,
    found: &mut HashSet<ExprId>,
) -> bool {
    let before = found.len();
    for i in 0..ast.exprs.len() {
        let call = ExprId(i as u32);
        if !matches!(helper_call(ast, call), Some(Some(_))) {
            continue;
        }
        let Expr::Call { args, .. } = ast.get_expr(call) else {
            continue;
        };
        if !found.contains(&call)
            && let Some(&ex) = args.first()
            && resolves_any(ast, ex, types)
        {
            found.insert(call);
        }
    }
    found.len() > before
}

/// May the executor `ex` settle its promise with a value of any kind?
/// Only an executor whose `resolve` is called directly in its own body,
/// each time with a value `types` shows as typed, settles a `T`: one
/// called from a closure the executor makes, handed on (`helper(r)`,
/// `const k = r`), or given a promise of `any` (§27.2.1.3.2 adopts it,
/// settling the value as it is) may settle anything, and so may an
/// executor whose body this walk cannot find.
fn resolves_any(ast: &Ast, ex: ExprId, types: &HashMap<ExprId, Type>) -> bool {
    let name = match ast.get_expr(ex) {
        Expr::Closure { fn_name, .. } => fn_name,
        Expr::Ident(n) => n,
        _ => return true,
    };
    let Some((params, body)) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name: n,
            params,
            body,
            ..
        } if n == name => Some((params, body)),
        _ => None,
    }) else {
        return true;
    };
    let Some(resolve) = params.iter().find(|p| p.name != "__env") else {
        return false;
    };
    let resolve = resolve.name.as_str();
    let mut stack: Vec<ExprId> = Vec::new();
    for s in body {
        visit_stmt_exprs(s, &mut |e| stack.push(e));
    }
    let mut seen = HashSet::new();
    while let Some(e) = stack.pop() {
        if !seen.insert(e) {
            continue;
        }
        match ast.get_expr(e) {
            Expr::Call { callee, args } if is_name(ast, *callee, resolve) => {
                if args.first().is_some_and(|a| any_settle(types.get(a))) {
                    return true;
                }
                stack.extend(args.iter().copied());
                continue;
            }
            Expr::Ident(n) if n == resolve => return true,
            Expr::Closure { captures, .. } if captures.iter().any(|c| c == resolve) => {
                return true;
            }
            _ => {}
        }
        visit_expr_children(ast, e, &mut |c| stack.push(c));
    }
    false
}

fn is_name(ast: &Ast, e: ExprId, name: &str) -> bool {
    matches!(ast.get_expr(e), Expr::Ident(n) if n == name)
}

/// Does resolving with a value of type `t` settle a value of any kind?
fn any_settle(t: Option<&Type>) -> bool {
    match t {
        Some(Type::Any) => true,
        Some(Type::Promise(inner)) => **inner == Type::Any,
        _ => false,
    }
}

/// Read each call in `calls` as `Promise<any>`: the call moves to a new
/// id under an `as Promise<any>`, which converts nothing (an assertion
/// hands the value on as it is).
pub(super) fn retype(r: &mut Ast, calls: &HashSet<ExprId>) {
    for c in calls {
        let call = r.exprs[c.0 as usize].clone();
        let span = r.expr_spans[c.0 as usize];
        let moved = r.add_expr(call);
        r.set_expr_span(moved, span);
        r.exprs[c.0 as usize] = Expr::As {
            expr: moved,
            ty_ann: "Promise<any>".to_string(),
        };
    }
}
