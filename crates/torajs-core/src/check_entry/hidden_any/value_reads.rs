//! The members a typed receiver reads as a value rather than calls.
//!
//! A method read as a value is handed to a slot typed with the return
//! its declaration spells, so a round that retypes the method's return
//! leaves the hierarchy of a method read that way as it is
//! (`super::method_rets`).

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, PropKey, Stmt};
use crate::check::Type;

/// `(class or interface, member)` of every member a receiver typed as
/// the class, or as an interface, reads as a value rather than calls.
/// A witness thunk's read is not one: the thunk is retyped with the
/// method (`MethodRets::ifaces`).
pub(super) fn value_reads(ast: &Ast, types: &HashMap<ExprId, Type>) -> HashSet<(String, PropKey)> {
    let mut calls: HashSet<ExprId> = (ast.exprs.iter())
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
    for (i, e) in ast.exprs.iter().enumerate() {
        let Expr::Member { obj, name } = e else {
            continue;
        };
        if calls.contains(&ExprId(i as u32)) {
            continue;
        }
        match types.get(obj) {
            Some(Type::ClassRef(c)) => {
                let c = c.split('<').next().unwrap_or(c);
                let mut at = Some(c);
                while let Some(k) = at {
                    out.insert((k.to_string(), PropKey::from(name)));
                    at = ast.class_parents.get(k).and_then(|p| p.as_deref());
                }
            }
            Some(t) => out.extend(
                (ast.iface_slots.types.iter())
                    .filter(|(_, it)| same_shape(t, it))
                    .map(|(i, _)| (i.clone(), PropKey::from(name))),
            ),
            None => {}
        }
    }
    out
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
