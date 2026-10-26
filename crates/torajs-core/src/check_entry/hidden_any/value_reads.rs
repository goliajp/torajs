//! The members a typed receiver reads as a value rather than calls.
//!
//! A method read as a value is handed to a slot typed with the return
//! its declaration spells, so a round that retypes the method's return
//! leaves the hierarchy of a method read that way as it is
//! (`super::method_rets`).

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, PropKey, Stmt};
use crate::check::Type;

/// `(class, member)` of every member a receiver typed as the class
/// reads as a value rather than calls. A witness thunk's read is not
/// one: the thunk is retyped with the method (`MethodRets::ifaces`).
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
        if let Expr::Member { obj, name } = e
            && !calls.contains(&ExprId(i as u32))
            && let Some(Type::ClassRef(c)) = types.get(obj)
        {
            let c = c.split('<').next().unwrap_or(c);
            let mut at = Some(c);
            while let Some(k) = at {
                out.insert((k.to_string(), PropKey::from(name)));
                at = ast.class_parents.get(k).and_then(|p| p.as_deref());
            }
        }
    }
    out
}
