//! The classes a field widened by name (`super::obj_fields`) leaves as
//! they are.
//!
//! A store of an `any` value through a receiver typed as class `H`
//! writes the slot of whatever object the receiver holds. That object
//! is an instance of `H`'s hierarchy unless a value of any kind entered
//! a slot typed with the hierarchy somewhere, and an instance of an
//! unrelated class `L` is among the values it may be only if `L`'s
//! instances are handed to a slot of another type somewhere (`any`,
//! `unknown`, an interface, a union, an array or object literal, a
//! callback). When either does not happen, `L`'s same-named field holds
//! only what its own stores give it, and keeps its declared type.
//!
//! Both questions are read off the places the expressions' values go
//! and a round's types; a place neither can read answers "yes".

use std::collections::{HashMap, HashSet};

use super::value_places::{Into, Place, into, places};
use crate::ast::{Ast, Expr, ExprId, PropKey, Stmt};
use crate::check::Type;
use crate::check_type_ann::strip_nullable;

/// The classes of a program whose field of the name stored a value of
/// any kind through receivers of the classes `owners` keeps its type,
/// `types` being a round's.
pub(super) fn kept(
    ast: &Ast,
    types: &HashMap<ExprId, Type>,
    owners: &HashSet<String>,
) -> HashSet<String> {
    let classes: Vec<&String> = ast.class_parents.keys().collect();
    let tree = |c: &str| -> HashSet<String> {
        let root = root_of(ast, c);
        (classes.iter())
            .filter(|k| root_of(ast, k) == root)
            .map(|k| (*k).clone())
            .collect()
    };
    let held: HashSet<String> = owners.iter().flat_map(|o| tree(o)).collect();
    let places = places(ast);
    let entered = entered_by_call(ast, &places, &held) || any_entered(ast, types, &places, &held);
    let mut out = HashSet::new();
    for c in classes.iter().copied() {
        if held.contains(c) || out.contains(c) {
            continue;
        }
        let t = tree(c);
        if !entered || !handed_on(ast, types, &places, &t) {
            out.extend(t);
        }
    }
    out
}

/// The values of the members of the object literals a class kept for
/// the member's name (`kept`) is laid out from: `let o: L = { f: .. }`.
pub(super) fn layout_values(
    ast: &Ast,
    kept: &HashMap<PropKey, HashSet<String>>,
) -> HashSet<ExprId> {
    let mut out = HashSet::new();
    if kept.values().all(HashSet::is_empty) {
        return out;
    }
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        if let Stmt::LetDecl {
            type_ann: Some(a),
            init,
            ..
        } = s
            && let Expr::ObjectLit { fields } = ast.get_expr(*init)
        {
            let class = base(strip_nullable(a).trim());
            out.extend(
                (fields.iter())
                    .filter(|(k, _)| kept.get(k).is_some_and(|cs| cs.contains(class)))
                    .map(|(_, v)| *v),
            );
        }
    });
    out
}

/// Whether a function some caller may reach without a call the checker
/// types takes a parameter typed with a class of `tree`: a method, a
/// closure or another compiler-made function, or a function read as a
/// value, whose arguments a boxed entry converts from values of any
/// kind. A method's receiver and `new.target`, and the parameters of a
/// method's all-`any` twin, are not among them.
fn entered_by_call(ast: &Ast, places: &HashMap<ExprId, Place>, tree: &HashSet<String>) -> bool {
    let read: HashSet<&str> = (ast.exprs.iter().enumerate())
        .filter_map(|(i, e)| match e {
            Expr::Ident(n) => Some((ExprId(i as u32), n.as_str())),
            _ => None,
        })
        .filter(|(e, _)| match places.get(e) {
            Some(Place::Expr(p)) => {
                !matches!(ast.get_expr(*p), Expr::Call { callee, .. } if callee == e)
            }
            _ => true,
        })
        .map(|(_, n)| n)
        .collect();
    let mut hit = false;
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        let Stmt::FnDecl { name, params, .. } = s else {
            return;
        };
        let dynamic = (name.starts_with("__") && !name.starts_with("__cmany_"))
            || read.contains(name.as_str());
        hit |= dynamic
            && (params.iter())
                .filter(|p| !matches!(p.name.as_str(), "__this" | "__env" | "__new_target"))
                .any(|p| p.type_ann.as_deref().is_none_or(|a| ann_mentions(a, tree)));
    });
    hit
}

/// Whether some value typed `any` enters a place typed with a class of
/// `tree`.
fn any_entered(
    ast: &Ast,
    types: &HashMap<ExprId, Type>,
    places: &HashMap<ExprId, Place>,
    tree: &HashSet<String>,
) -> bool {
    let named = |t: &Type| mentions(t, tree);
    (0..ast.exprs.len()).map(|i| ExprId(i as u32)).any(|e| {
        if !types.get(&e).is_some_and(carries_any) {
            return false;
        }
        match places.get(&e) {
            None | Some(Place::Other) => true,
            Some(Place::Drop) | Some(Place::Bind(None)) => false,
            Some(Place::Bind(Some(a))) => ann_mentions(a, tree),
            Some(Place::Ret(a)) => a.as_deref().is_none_or(|a| ann_mentions(a, tree)),
            Some(Place::Expr(p)) => match into(ast, types, *p, e) {
                Into::Read => false,
                Into::Slot(Some(t)) | Into::Builtin(t) => named(t),
                Into::Ann(a) => ann_mentions(a, tree),
                Into::Slot(None) | Into::Unknown => true,
                Into::Through => types.get(p).is_none_or(|t| named(t) && !carries_any(t)),
            },
        }
    })
}

/// Whether some value typed with a class of `tree` is handed to a
/// place of another type.
fn handed_on(
    ast: &Ast,
    types: &HashMap<ExprId, Type>,
    places: &HashMap<ExprId, Place>,
    tree: &HashSet<String>,
) -> bool {
    (0..ast.exprs.len()).map(|i| ExprId(i as u32)).any(|e| {
        let Some(t) = types.get(&e).filter(|t| mentions(t, tree)) else {
            return false;
        };
        // a container of the instances keeps them only where its own
        // type goes unchanged
        let same = |s: &Type| {
            if is_of(t, tree) {
                is_of(s, tree)
            } else {
                s == t
            }
        };
        let same_ann = |a: &str| is_of(t, tree) && ann_is(a, tree);
        match places.get(&e) {
            None | Some(Place::Other) | Some(Place::Ret(None)) => true,
            Some(Place::Drop) | Some(Place::Bind(None)) => false,
            Some(Place::Bind(Some(a))) | Some(Place::Ret(Some(a))) => !same_ann(a),
            Some(Place::Expr(p)) => match into(ast, types, *p, e) {
                Into::Read => false,
                Into::Slot(Some(s)) => !same(s),
                Into::Ann(a) => !same_ann(a),
                Into::Slot(None) | Into::Builtin(_) | Into::Unknown => true,
                Into::Through => types.get(p).is_none_or(|s| !same(s)),
            },
        }
    })
}

/// Whether a value typed `t` may hold a value of any kind.
fn carries_any(t: &Type) -> bool {
    match t {
        Type::Any => true,
        Type::Array(e) | Type::Nullable(e) => carries_any(e),
        Type::Union(ts) => ts.iter().any(carries_any),
        _ => false,
    }
}

/// Whether `t` names a class of `tree`, at any depth.
fn mentions(t: &Type, tree: &HashSet<String>) -> bool {
    match t {
        Type::ClassRef(c) => tree.contains(base(c)),
        Type::Struct(fs) => fs.iter().any(|(_, f)| mentions(f, tree)),
        Type::Function(ps, r) => mentions(r, tree) || ps.iter().any(|p| mentions(p, tree)),
        Type::Array(e) | Type::Nullable(e) | Type::Rest(e) | Type::MapIter(e) => mentions(e, tree),
        Type::Map(k, v) => mentions(k, tree) || mentions(v, tree),
        Type::Union(ts) => ts.iter().any(|t| mentions(t, tree)),
        _ => false,
    }
}

/// Whether `t` is a class of `tree`, or null.
fn is_of(t: &Type, tree: &HashSet<String>) -> bool {
    match t {
        Type::ClassRef(c) => tree.contains(base(c)),
        Type::Nullable(t) => is_of(t, tree),
        _ => false,
    }
}

/// Whether the annotation `a` spells a class of `tree`, or null.
fn ann_is(a: &str, tree: &HashSet<String>) -> bool {
    tree.contains(base(strip_nullable(a).trim()))
}

/// Whether the annotation `a` names a class of `tree` anywhere in it.
fn ann_mentions(a: &str, tree: &HashSet<String>) -> bool {
    (a.split(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '$')))
        .any(|w| tree.contains(w))
}

fn base(c: &str) -> &str {
    c.split('<').next().unwrap_or(c)
}

fn root_of<'a>(ast: &'a Ast, mut c: &'a str) -> &'a str {
    while let Some(Some(p)) = ast.class_parents.get(c) {
        c = p;
    }
    c
}
