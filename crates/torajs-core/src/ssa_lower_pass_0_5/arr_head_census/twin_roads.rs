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
pub(crate) use idle::TwinBodies;

/// A member or string name that reaches a method value or moves one
/// onto another receiver.
fn rebinds(n: &str) -> bool {
    matches!(
        n,
        "call"
            | "apply"
            | "bind"
            | "prototype"
            | "__proto__"
            | "constructor"
            | "setPrototypeOf"
            | "getPrototypeOf"
            | "create"
            | "Reflect"
            | "eval"
            | "Function"
    )
}

/// A set of the program's expressions, one flag per arena slot.
#[derive(Default)]
pub(crate) struct ExprMask(Vec<bool>);

impl ExprMask {
    fn new(ast: &Ast) -> Self {
        ExprMask(vec![false; ast.exprs.len()])
    }

    pub(crate) fn contains(&self, e: ExprId) -> bool {
        self.0.get(e.0 as usize).is_some_and(|b| *b)
    }

    /// Add `e`; false when it was there already.
    fn insert(&mut self, e: ExprId) -> bool {
        !std::mem::replace(&mut self.0[e.0 as usize], true)
    }

    pub(crate) fn is_empty(&self) -> bool {
        !self.0.contains(&true)
    }
}

/// The expressions of every twin body, or nothing when some road can
/// hand a twin a foreign receiver.
pub(crate) fn unreachable_twin_exprs(
    ast: &Ast,
    expr_types: &std::collections::HashMap<ExprId, Type>,
) -> ExprMask {
    let Some(methods) = twinned_methods(ast) else {
        return ExprMask::default();
    };

    if methods.is_empty() {
        return ExprMask::default();
    }
    let twins: HashSet<&str> = ast.cmany_twins.values().map(String::as_str).collect();
    let mut inside = ExprMask::new(ast);
    for s in &ast.stmts {
        collect_twin_bodies(ast, s, &twins, &mut inside);
    }
    let callees = callees(ast);
    let road = ast.exprs.iter().enumerate().any(|(i, e)| {
        let id = ExprId(i as u32);
        !inside.contains(id) && is_road(e, id, (&methods, &callees), expr_types, false)
    });
    if road { ExprMask::default() } else { inside }
}

/// Every callee of a call. A `this.m(..)` call rewritten to the static
/// `__cm_` form keeps its member callee on the side, still a callee.
fn callees(ast: &Ast) -> ExprMask {
    let mut out = ExprMask::new(ast);
    let calls = ast.exprs.iter().filter_map(|e| match e {
        Expr::Call { callee, .. } | Expr::OptCall { callee, .. } => Some(*callee),
        _ => None,
    });
    for c in calls.chain(ast.cm_this_static_calls.values().copied()) {
        out.insert(c);
    }
    out
}

/// The method names that have a twin; `None` when one cannot be read
/// back off its mangled name or its class has a subclass.
fn twinned_methods(ast: &Ast) -> Option<HashSet<String>> {
    Some(twinned(ast, false)?.into_iter().map(|(_, m)| m).collect())
}

/// Every mono method with a twin, with the method's name; `None` as
/// for [`twinned_methods`]. With `subclassed`, a class whose every
/// subclass lays out its fields as it does ([`subclasses_agree`]) keeps
/// its twins: a subclass instance the twin stores into holds those
/// fields in slots of the types the mono stores.
fn twinned(ast: &Ast, subclassed: bool) -> Option<Vec<(&str, String)>> {
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
            && !(subclassed && subclasses_agree(ast, class))
        {
            return None;
        }
        out.push((mono.as_str(), rest[class.len() + 2..].to_string()));
    }
    Some(out)
}

/// Whether every class descending from `class` declares each data
/// field `class` declares with the same annotation. An accessor's slots
/// (`__getter_v` / `__setter_v`) are no data field a write lands in,
/// and an override respells them.
fn subclasses_agree(ast: &Ast, class: &str) -> bool {
    use crate::ast_throw_info::obj_cast::decl_fields;
    let Some((_, own)) = decl_fields(ast, class) else {
        return false;
    };
    let data = |k: &crate::ast::PropKey| {
        let k = k.lossy();
        !k.starts_with("__getter_") && !k.starts_with("__setter_")
    };
    let mut subclasses =
        (ast.class_parents.keys()).filter(|c| *c != class && idle::descends(ast, c, class));
    subclasses.all(|sub| {
        decl_fields(ast, sub).is_some_and(|(_, fields)| {
            (fields.iter().filter(|(k, _)| data(k)))
                .all(|(k, a)| own.iter().all(|(o, oa)| o != k || oa == a))
        })
    })
}

fn collect_twin_bodies(ast: &Ast, s: &Stmt, twins: &HashSet<&str>, out: &mut ExprMask) {
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

fn add_tree(ast: &Ast, e: ExprId, out: &mut ExprMask) {
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
    (methods, callees): (&HashSet<String>, &ExprMask),
    expr_types: &std::collections::HashMap<ExprId, Type>,
    reached_only: bool,
) -> bool {
    untyped_road(e, id, (methods, callees)) || typed_road(e, id, expr_types, reached_only)
}

/// The roads the types play no part in: a name.
fn untyped_road(e: &Expr, id: ExprId, (methods, callees): (&HashSet<String>, &ExprMask)) -> bool {
    match e {
        Expr::Ident(n) => rebinds(n),
        Expr::Member { name, .. } | Expr::OptChain { name, .. } => {
            rebinds(name) || (!callees.contains(id) && methods.contains(name))
        }
        Expr::String(s) => {
            s.as_str().is_some_and(rebinds) || methods.iter().any(|m| s == m.as_str())
        }
        _ => false,
    }
}

/// The roads the types decide: a computed read.
fn typed_road(
    e: &Expr,
    id: ExprId,
    expr_types: &std::collections::HashMap<ExprId, Type>,
    reached_only: bool,
) -> bool {
    match e {
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
