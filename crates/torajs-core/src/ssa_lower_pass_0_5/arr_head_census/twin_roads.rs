//! Which expressions the census may leave out: the bodies of the
//! receiver-polymorphic method twins (`__cmany_<C>__<m>`), when no
//! foreign receiver can ever reach one.
//!
//! A twin is the same source body as its mono method with every
//! param, the receiver included, typed `any`, so each `this.xs[this.i]`
//! in it reads as a computed key on an `any` receiver and the census
//! would count it. The boxed adapter enters a twin only for a receiver
//! that is not a `C` instance, and a method reaches such a receiver
//! only as a value: read off an instance or a prototype other than as
//! the callee of a call, named by a string, read through a computed
//! key that may spell it, rebound with `call` / `apply` / `bind` /
//! `Reflect`, placed behind another prototype, or inherited by a
//! subclass instance (whose class tag is not `C`'s). The walk below
//! looks for each of those roads syntactically, over every expression
//! outside the twins; finding any keeps every twin in the census.
//! Inside the twins the same code also exists in the mono body, typed,
//! so a road that starts there is found there.

use std::collections::HashSet;

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::Type;

mod idle;
pub(crate) use idle::idle_twin_exprs;

/// Member or string names that reach a method value or move one onto
/// another receiver.
const REBIND_NAMES: &[&str] = &[
    "call",
    "apply",
    "bind",
    "prototype",
    "__proto__",
    "constructor",
    "setPrototypeOf",
    "getPrototypeOf",
    "create",
    "Reflect",
    "eval",
    "Function",
];

/// The expressions of every twin body, or nothing when some road can
/// hand a twin a foreign receiver.
pub(crate) fn unreachable_twin_exprs(
    ast: &Ast,
    expr_types: &std::collections::HashMap<ExprId, Type>,
) -> HashSet<ExprId> {
    let Some(methods) = twinned_methods(ast) else {
        return HashSet::new();
    };

    if methods.is_empty() {
        return HashSet::new();
    }
    let twins: HashSet<&str> = ast.cmany_twins.values().map(String::as_str).collect();
    let mut inside: HashSet<ExprId> = HashSet::new();
    for s in &ast.stmts {
        collect_twin_bodies(ast, s, &twins, &mut inside);
    }
    let callees = callees(ast);
    let road = ast.exprs.iter().enumerate().any(|(i, e)| {
        let id = ExprId(i as u32);
        !inside.contains(&id) && is_road(e, id, (&methods, &callees), expr_types, false)
    });
    if road { HashSet::new() } else { inside }
}

/// Every callee of a call. A `this.m(..)` call rewritten to the static
/// `__cm_` form keeps its member callee on the side, still a callee.
fn callees(ast: &Ast) -> HashSet<ExprId> {
    (ast.exprs.iter())
        .filter_map(|e| match e {
            Expr::Call { callee, .. } | Expr::OptCall { callee, .. } => Some(*callee),
            _ => None,
        })
        .chain(ast.cm_this_static_calls.values().copied())
        .collect()
}

/// The method names that have a twin; `None` when one cannot be read
/// back off its mangled name or its class has a subclass.
fn twinned_methods(ast: &Ast) -> Option<HashSet<String>> {
    Some(twinned(ast)?.into_iter().map(|(_, m)| m).collect())
}

/// Every mono method with a twin, with the method's name; `None` as
/// for [`twinned_methods`].
fn twinned(ast: &Ast) -> Option<Vec<(&str, String)>> {
    let mut out = Vec::new();
    for mono in ast.cmany_twins.keys() {
        let rest = mono.strip_prefix("__cm_")?;
        let class = ast
            .class_parents
            .keys()
            .filter(|c| {
                rest.strip_prefix(c.as_str())
                    .is_some_and(|r| r.starts_with("__"))
            })
            .max_by_key(|c| c.len())?;
        if ast
            .class_parents
            .values()
            .any(|p| p.as_deref() == Some(class.as_str()))
        {
            return None;
        }
        out.push((mono.as_str(), rest[class.len() + 2..].to_string()));
    }
    Some(out)
}

fn collect_twin_bodies(ast: &Ast, s: &Stmt, twins: &HashSet<&str>, out: &mut HashSet<ExprId>) {
    match s {
        Stmt::FnDecl {
            name, params, body, ..
        } if twins.contains(name.as_str()) => {
            let mut add = |e: ExprId| add_tree(ast, e, out);
            for p in params {
                if let Some(d) = p.default {
                    add(d);
                }
            }
            for st in body {
                visit_stmt_exprs(st, &mut add);
            }
        }
        Stmt::Multi(v) | Stmt::Block(v) => {
            for st in v {
                collect_twin_bodies(ast, st, twins, out);
            }
        }
        _ => {}
    }
}

fn add_tree(ast: &Ast, e: ExprId, out: &mut HashSet<ExprId>) {
    if out.insert(e) {
        visit_expr_children(ast, e, &mut |c| add_tree(ast, c, out));
    }
}

/// Whether `e` is a road. With `reached_only`, a computed read the
/// checker never reached, key and all (the array lane a for-of over an
/// iterator leaves behind), is not one: it is never performed.
fn is_road(
    e: &Expr,
    id: ExprId,
    (methods, callees): (&HashSet<String>, &HashSet<ExprId>),
    expr_types: &std::collections::HashMap<ExprId, Type>,
    reached_only: bool,
) -> bool {
    match e {
        Expr::Ident(n) => REBIND_NAMES.contains(&n.as_str()),
        Expr::Member { name, .. } | Expr::OptChain { name, .. } => {
            REBIND_NAMES.contains(&name.as_str())
                || (methods.contains(name) && !callees.contains(&id))
        }
        Expr::String(s) => {
            REBIND_NAMES.iter().any(|n| s == *n) || methods.iter().any(|m| s == m.as_str())
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            (!reached_only || expr_types.contains_key(&id) || expr_types.contains_key(index))
                && !key_is_numeric(expr_types.get(index))
                && !never_an_instance(expr_types.get(obj))
        }
        _ => false,
    }
}

fn key_is_numeric(t: Option<&Type>) -> bool {
    matches!(
        t,
        Some(Type::Number | Type::Boolean | Type::BigInt | Type::Null | Type::Undefined)
    )
}

/// A value of this type is never a class instance.
fn never_an_instance(t: Option<&Type>) -> bool {
    matches!(
        t,
        Some(
            Type::Array(_)
                | Type::String
                | Type::Number
                | Type::Boolean
                | Type::BigInt
                | Type::Symbol
                | Type::Null
                | Type::Undefined
        )
    )
}
