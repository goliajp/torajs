//! The computed keys of the `then` closure proof.

use std::collections::HashMap;

use super::forms::danger_key;
use super::{Opening, open};
use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Lit, Type};

/// A computed member key: a literal, or a key the checker typed as one
/// that cannot name a dangerous property.
pub(super) fn key_opening(
    ast: &Ast,
    index: ExprId,
    types: Option<&HashMap<ExprId, Type>>,
) -> Option<Opening> {
    match ast.get_expr(index) {
        Expr::Number(_) | Expr::Bool(_) | Expr::BigInt { .. } => None,
        Expr::String(s) => match s.as_wtf8().as_str() {
            Some(k) if !danger_key(k) => None,
            _ => open("computed key", s.to_string_lossy_owned()),
        },
        _ => {
            let types = types?;
            match types.get(&index) {
                Some(t) if key_type_known(t) => None,
                _ => open("computed key", ""),
            }
        }
    }
}

/// A key of type `t` is a number, a boolean, a symbol, or a string
/// literal that names no dangerous property.
fn key_type_known(t: &Type) -> bool {
    match t {
        Type::Number | Type::BigInt | Type::Symbol | Type::Boolean => true,
        Type::Literal(Lit::Str(s)) => s.as_wtf8().as_str().is_some_and(|k| !danger_key(k)),
        Type::Literal(Lit::Num(_) | Lit::Bool(_)) => true,
        Type::Union(members) => members.iter().all(key_type_known),
        _ => false,
    }
}

/// A call whose callee the walk must read with its arguments: the
/// `super` property kernels take their key as the second argument.
pub(super) fn call_opening(ast: &Ast, callee: ExprId, args: &[ExprId]) -> Option<Opening> {
    match ast.get_expr(callee) {
        Expr::Ident(n) if n.starts_with("__torajs_super_prop_") => {
            match args.get(1).map(|&k| ast.get_expr(k)) {
                Some(Expr::String(s)) if s.as_wtf8().as_str().is_some_and(|k| !danger_key(k)) => {
                    None
                }
                _ => open("super key", n.clone()),
            }
        }
        _ => None,
    }
}
