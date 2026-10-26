//! The members a typed receiver reads as a value rather than calls.
//!
//! A method read as a value is handed to a slot typed with the return
//! its declaration spells, so a round that retypes the method's return
//! leaves the hierarchy of a method read that way as it is
//! (`super::method_rets`). A read that only compares the function,
//! inspects it or looks a collection's key up with it never calls it,
//! and no slot reads its answer: §7.2.10 SameValue and the function's
//! own properties do not depend on the return its declaration spells.
//! A store into a collection is one only where the collection hands
//! the function back to no slot typed with a return: a `Map` key typed
//! `() => number` is read back as that type and called.

use std::collections::{HashMap, HashSet};

use super::value_places::{Place, places};
use crate::ast::{Ast, BinOp, Expr, ExprId, PropKey, Stmt};
use crate::check::Type;

/// `(class or interface, member)` of every member a receiver typed as
/// the class, or as an interface, reads as a value rather than calls,
/// in `ast` or in a specialization `mono` adds past it: a generic body
/// is typed only in its specializations. A witness thunk's read is not
/// one: the thunk is retyped with the method (`MethodRets::ifaces`).
pub(super) fn value_reads(
    (ast, mono): (&Ast, &Ast),
    types: &HashMap<ExprId, Type>,
) -> HashSet<(String, PropKey)> {
    let mut calls: HashSet<ExprId> = (ast.exprs.iter().chain(&mono.exprs[ast.exprs.len()..]))
        .filter_map(|e| match e {
            Expr::Call { callee, .. } => Some(*callee),
            _ => None,
        })
        .collect();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, body, .. } = s
            && (name.starts_with("__iw_") || name.starts_with("__ilit_"))
        {
            calls.extend(super::promise_rets::own_returns(body));
        }
    }
    let mut out = HashSet::new();
    let mut reads = |of: &Ast, from: usize| {
        let mut at = None;
        for (i, e) in of.exprs.iter().enumerate().skip(from) {
            let Expr::Member { obj, name } = e else {
                continue;
            };
            let read = ExprId(i as u32);
            if calls.contains(&read) {
                continue;
            }
            let owners = owners(ast, types.get(obj));
            if owners.is_empty() || inspected(of, types, at.get_or_insert_with(|| places(of)), read)
            {
                continue;
            }
            out.extend(owners.into_iter().map(|k| (k, PropKey::from(name))));
        }
    };
    reads(ast, 0);
    reads(mono, ast.exprs.len());
    out
}

/// The class and its ancestors a receiver typed `t` reads a member
/// of, or the interfaces of its shape.
fn owners(ast: &Ast, t: Option<&Type>) -> Vec<String> {
    match t {
        Some(Type::ClassRef(c)) => {
            let mut out = Vec::new();
            let mut at = Some(c.split('<').next().unwrap_or(c));
            while let Some(k) = at {
                out.push(k.to_string());
                at = ast.class_parents.get(k).and_then(|p| p.as_deref());
            }
            out
        }
        Some(t) => (ast.iface_slots.types.iter())
            .filter(|(_, it)| same_shape(t, it))
            .map(|(i, _)| i.clone())
            .collect(),
        None => Vec::new(),
    }
}

/// Whether the value `read` goes only into a comparison, an operator,
/// `typeof`, a property read other than `call` / `apply` / `bind`, or
/// the key argument of a `Map` / `Set` lookup, or of a store whose
/// collection never hands the key back typed with a return.
fn inspected(
    ast: &Ast,
    types: &HashMap<ExprId, Type>,
    places: &HashMap<ExprId, Place>,
    read: ExprId,
) -> bool {
    let Some(Place::Expr(p)) = places.get(&read) else {
        return false;
    };
    match ast.get_expr(*p) {
        Expr::BinOp { op, .. } => !matches!(op, BinOp::LAnd | BinOp::LOr),
        Expr::Unary { .. } | Expr::TypeOf { .. } => true,
        Expr::Member { name, .. } => !matches!(name.as_str(), "call" | "apply" | "bind"),
        Expr::Call { callee, args } => {
            let Expr::Member { obj, name } = ast.get_expr(*callee) else {
                return false;
            };
            args.first() == Some(&read)
                && match (name.as_str(), types.get(obj)) {
                    (
                        "get" | "has" | "delete",
                        Some(Type::Map(..) | Type::Set | Type::WeakMap | Type::WeakSet),
                    ) => true,
                    // a set hands its elements back as `any` values, and a
                    // weak collection never hands its keys back
                    ("add", Some(Type::Set | Type::WeakSet)) | ("set", Some(Type::WeakMap)) => true,
                    ("set", Some(Type::Map(key, _))) => !returns_typed(key),
                    _ => false,
                }
        }
        _ => false,
    }
}

/// Whether a slot typed `t` calls the function it holds through a
/// declared return other than `any`.
fn returns_typed(t: &Type) -> bool {
    match non_null(t) {
        Type::Function(_, ret) => **ret != Type::Any,
        _ => false,
    }
}

/// Whether a receiver typed `t` may be a value of the interface typed
/// `iface`: the same members, a method's return aside, which a round
/// may have retyped on one side only.
fn same_shape(t: &Type, iface: &Type) -> bool {
    let (Type::Struct(a), Type::Struct(b)) = (non_null(t), iface) else {
        return false;
    };
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|((ka, ta), (kb, tb))| {
            ka == kb
                && match (non_null(ta), non_null(tb)) {
                    (Type::Function(pa, _), Type::Function(pb, _)) => {
                        pa == pb
                            && matches!(ta, Type::Nullable(_)) == matches!(tb, Type::Nullable(_))
                    }
                    _ => ta == tb,
                }
        })
}

/// `t` without the `Nullable` an optional member or receiver wraps it in.
fn non_null(t: &Type) -> &Type {
    match t {
        Type::Nullable(t) => t,
        t => t,
    }
}

#[cfg(test)]
mod tests {
    use super::same_shape;
    use crate::ast::PropKey;
    use crate::check::Type;

    fn iface(m: Type) -> Type {
        Type::struct_of(vec![
            (PropKey::from("m"), m),
            (PropKey::from("n"), Type::Number),
        ])
    }

    fn method(ret: Type) -> Type {
        Type::Function(vec![], Box::new(ret))
    }

    #[test]
    fn a_retyped_return_keeps_the_shape() {
        let (was, now) = (method(Type::Number), method(Type::Any));
        assert!(same_shape(&iface(now), &iface(was)));
    }

    #[test]
    fn an_optional_method_keeps_the_shape_when_its_return_is_retyped() {
        let was = Type::Nullable(Box::new(method(Type::Number)));
        let now = Type::Nullable(Box::new(method(Type::Any)));
        assert!(same_shape(&iface(now), &iface(was)));
    }

    #[test]
    fn an_optional_method_is_not_a_required_one() {
        let opt = Type::Nullable(Box::new(method(Type::Number)));
        assert!(!same_shape(&iface(method(Type::Number)), &iface(opt)));
    }
}
