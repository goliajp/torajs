//! The object type literals that spell an interface whose method a
//! round retypes to return `any`.
//!
//! A literal written in place (`o: { m(): number }`) checks as the
//! declaration of its shape, the one `iface_witness::inline` makes for
//! it or one the program declares: a call through it dispatches by the
//! rows of the declaration whose type is equal to its own. Once the
//! declaration's member returns `any`, a literal still spelling the old
//! return would be equal to no declaration, and a class instance would
//! no longer be one of its values. Every literal of the declaration's
//! shape is respelled with it.

use std::collections::{HashMap, HashSet};

use super::inlobj_fields;
use super::method_rets::{Family, returning_any};
use crate::ast::{Ast, Expr, ExprId, PropKey, Stmt};
use crate::check::Type;

/// A declaration's members `(name, annotation)` in declaration order,
/// and the names of those whose return is retyped. The order is part of
/// the shape: a literal whose members come in another order checks as
/// a declaration of its own (`IfaceSlots::iface_of` compares the types
/// member by member).
type Shape = (Vec<(String, String)>, HashSet<String>);

/// Respell every object type literal of the shape of an interface with
/// a member in `fam.members`, before the interface itself is retyped.
pub(super) fn retype(r: &mut Ast, fam: &Family) {
    let shapes = shapes(r, fam);
    if shapes.is_empty() {
        return;
    }
    inlobj_fields::respell(r, &|ms| {
        let shape: Vec<(String, String)> = (ms.iter())
            .map(|(k, a)| (k.trim().to_string(), a.trim().to_string()))
            .collect();
        let keys: HashSet<&String> = (shapes.iter())
            .filter(|(s, _)| *s == shape)
            .flat_map(|(_, ks)| ks)
            .collect();
        if keys.is_empty() {
            return None;
        }
        let member = |&(k, a): &(&str, &str)| match returning_any(a.trim()) {
            Some(n) if keys.contains(&k.trim().to_string()) => format!("{k}:{n}"),
            _ => format!("{k}:{a}"),
        };
        Some(ms.iter().map(member).collect())
    });
}

/// The shapes of the non-generic declarations with a member in
/// `fam.members`, as `r` spells them now.
fn shapes(r: &Ast, fam: &Family) -> Vec<Shape> {
    let mut out = Vec::new();
    for s in &r.stmts {
        let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = s
        else {
            continue;
        };
        if !type_params.is_empty() {
            continue;
        }
        let keys: HashSet<String> = (fields.iter())
            .filter(|(k, _)| fam.members.contains(&(name.clone(), k.clone())))
            .filter_map(|(k, _)| k.as_str().map(str::to_string))
            .collect();
        let shape = (fields.iter())
            .map(|(k, a)| Some((k.as_str()?.to_string(), a.trim().to_string())))
            .collect::<Option<Vec<_>>>();
        if let Some(shape) = shape.filter(|_| !keys.is_empty()) {
            out.push((shape, keys));
        }
    }
    out
}

/// The lifted closures that object literals of the type of an interface
/// in `fam.members` hold in a retyped member: the closure is the
/// member's slot, called through the interface's signature, so it
/// returns its value as it is with the interface. `types` is a round's;
/// a literal's type is the interface's before the retype, or after it
/// once a previous round retyped its closures.
pub(super) fn closures(ast: &Ast, types: &HashMap<ExprId, Type>, fam: &Family) -> HashSet<String> {
    let mut out = HashSet::new();
    if fam.members.is_empty() {
        return out;
    }
    let ifaces: Vec<(&Type, HashSet<&PropKey>)> = (ast.iface_slots.types.iter())
        .filter_map(|(i, t)| {
            let keys: HashSet<&PropKey> = (fam.members.iter())
                .filter(|(n, _)| n == i)
                .map(|(_, k)| k)
                .collect();
            (!keys.is_empty()).then_some((t, keys))
        })
        .collect();
    for (i, e) in ast.exprs.iter().enumerate() {
        let Expr::ObjectLit { fields } = e else {
            continue;
        };
        let Some(t) = types.get(&ExprId(i as u32)) else {
            continue;
        };
        let Some((_, keys)) = ifaces.iter().find(|(it, ks)| of_iface(t, it, ks)) else {
            continue;
        };
        for (k, v) in fields {
            if keys.contains(k)
                && let Expr::Closure { fn_name, .. } = ast.get_expr(*v)
            {
                out.insert(fn_name.clone());
            }
        }
    }
    out
}

/// Whether `t` is `iface`, or `iface` with the members `keys` returning
/// `any`.
fn of_iface(t: &Type, iface: &Type, keys: &HashSet<&PropKey>) -> bool {
    let (Type::Struct(a), Type::Struct(b)) = (t, iface) else {
        return false;
    };
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|((ka, ta), (kb, tb))| {
            ka == kb && (ta == tb || keys.contains(ka) && returns_any_of(ta, tb))
        })
}

/// Whether `ta` is the function type `tb` returning `any`.
fn returns_any_of(ta: &Type, tb: &Type) -> bool {
    match (ta, tb) {
        (Type::Function(pa, ra), Type::Function(pb, _)) => pa == pb && **ra == Type::Any,
        (Type::Nullable(a), Type::Nullable(b)) => returns_any_of(a, b),
        _ => false,
    }
}
