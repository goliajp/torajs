//! A promise binding whose `then` the program stores is an `any`.
//!
//! `let p = Promise.resolve(); p.then = function (r, j) { … }` — the
//! test262 `invoke-then` / `await-monkey-patched-promise` shape — is a
//! property WRITE on a promise: an own `then` entry in the cell's
//! expando bag, which the any lane's own-bag gate, the combinators'
//! per-element `Invoke(nextPromise, "then")` and the bound-builtin
//! re-dispatch all already honour. The typed `Promise<T>` binding the
//! bare `let` infers has no such surface: the store is the checker's
//! `field assignment target must be a struct` reject, and a
//! `this`-reading function expression stored there died one gate
//! earlier as an unclaimed receiver (rotation 651, both measured).
//! The same program with `: any` spelled on the binding answers bun
//! line for line — so the annotation becomes what the user could have
//! written, the way `objlit_nominal_widen` / `with_object_widen` do it.
//!
//! Admission is syntactic on both ends. The BINDING's init must be a
//! certain promise — `new Promise(…)` or a `Promise.<static>(…)`
//! call, `Promise` unshadowed — and the program must hold a `then`
//! store on that NAME: `name.then = …` (a chained `a.then = b.then =
//! f` is one Assign per name) or `Object.defineProperty(name, "then",
//! …)`. Names are not scope-resolved: a same-named binding elsewhere
//! widens this one, which costs the typed lane's direct dispatch on a
//! binding whose `then` the program never actually touched — never a
//! wrong answer.
//!
//! Pipeline: beside the other declaration widens, before any pass
//! reads the binding's type and before `desugar_builtin_new` turns
//! `new Promise(…)` into its factory call.

use std::collections::HashSet;

use super::{Ast, Expr, ExprId, Stmt};

/// The statics whose answer is a promise (§27.2.4; `withResolvers`
/// answers a record and is deliberately absent).
const PROMISE_STATICS: [&str; 7] = [
    "resolve",
    "reject",
    "all",
    "allSettled",
    "any",
    "race",
    "try",
];

pub fn widen_promise_then_store_bindings(ast: &mut Ast) {
    let exprs = &ast.exprs;
    let mut names: HashSet<String> = HashSet::new();
    for e in exprs {
        match e {
            Expr::Assign { target, .. } => {
                if let Expr::Member { obj, name } = &exprs[target.0 as usize]
                    && name == "then"
                    && let Expr::Ident(n) = &exprs[peel_as(exprs, *obj).0 as usize]
                {
                    names.insert(n.clone());
                }
            }
            Expr::Call { callee, args } => {
                if let Expr::Member { obj, name } = &exprs[callee.0 as usize]
                    && name == "defineProperty"
                    && matches!(&exprs[obj.0 as usize], Expr::Ident(o) if o == "Object")
                    && args.len() >= 2
                    && let Expr::Ident(n) = &exprs[peel_as(exprs, args[0]).0 as usize]
                    && matches!(&exprs[args[1].0 as usize], Expr::String(s) if *s == "then")
                {
                    names.insert(n.clone());
                }
            }
            _ => {}
        }
    }
    if names.is_empty() || super::fnexpr_this_names::name_shadowed_elsewhere(&ast.stmts, "Promise")
    {
        return;
    }
    let mut stmts = std::mem::take(&mut ast.stmts);
    widen_flat(&mut stmts, &ast.exprs, &names);
    for s in stmts.iter_mut() {
        super::stmt_nested_lists::for_each_nested_vec_mut(s, &mut |inner| {
            widen_flat(inner, &ast.exprs, &names)
        });
    }
    ast.stmts = stmts;
}

fn widen_flat(stmts: &mut [Stmt], exprs: &[Expr], names: &HashSet<String>) {
    for s in stmts.iter_mut() {
        let Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } = s
        else {
            continue;
        };
        if type_ann.is_some() || !names.contains(name) {
            continue;
        }
        if certain_promise_init(exprs, *init) {
            *type_ann = Some("any".to_string());
        }
    }
}

/// `new Promise(…)` or `Promise.<static>(…)`, through `as` casts.
fn certain_promise_init(exprs: &[Expr], init: ExprId) -> bool {
    match &exprs[peel_as(exprs, init).0 as usize] {
        Expr::New { class_name, .. } => class_name == "Promise",
        Expr::Call { callee, .. } => matches!(&exprs[callee.0 as usize], Expr::Member { obj, name }
            if PROMISE_STATICS.contains(&name.as_str())
                && matches!(&exprs[obj.0 as usize], Expr::Ident(p) if p == "Promise")),
        _ => false,
    }
}

fn peel_as(exprs: &[Expr], mut e: ExprId) -> ExprId {
    while let Expr::As { expr, .. } = &exprs[e.0 as usize] {
        e = *expr;
    }
    e
}
