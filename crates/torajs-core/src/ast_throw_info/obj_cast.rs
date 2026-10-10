//! The boundaries where an `any` value crosses into an object-layout
//! slot the program annotated raise a TypeError when the value is not
//! a cell the slot can read (`ssa_lower_checked_cast::obj`): a `let`
//! initializer, an assignment, a return. The lowering decides each
//! one from the slot's annotation; this answers the same question
//! from the source, erring towards "may raise".

use crate::ast::{Ast, ExprId, Stmt};
use crate::check::Type;
use crate::check_type_ann::strip_nullable;
use std::collections::HashMap;

/// Whether `ann` names an object type: a class, an object type literal
/// or a declared type of fields.
pub(crate) fn object_ann(ast: &Ast, ann: &str) -> bool {
    let base = strip_nullable(ann).trim();
    base.starts_with('{')
        || ast.class_parents.contains_key(base)
        || ast.stmts.iter().any(|s| {
            matches!(s, Stmt::TypeDecl { name, type_params, .. }
                if name == base && type_params.is_empty())
        })
}

/// Whether a binding of checker type `t` is an object layout.
pub(crate) fn object_ty(t: Option<&Type>) -> bool {
    match t {
        Some(Type::Nullable(t)) => object_ty(Some(t)),
        Some(Type::Struct(_) | Type::ClassRef(_)) => true,
        _ => false,
    }
}

/// Whether the function `name`, declared returning `ret`, returns an
/// `any` value into an object return type it wrote.
pub(crate) fn returns_any_into_obj(
    ast: &Ast,
    name: &str,
    ret: Option<&str>,
    body: &[Stmt],
    expr_types: &HashMap<ExprId, Type>,
) -> bool {
    if !ret.is_some_and(|r| object_ann(ast, r)) || ast.contextual_ret_fns.contains(name) {
        return false;
    }
    let mut hit = false;
    crate::ast::walk_stmts(body, &mut |s| {
        if let Stmt::Return(Some(e)) = s
            && expr_types.get(e) == Some(&Type::Any)
        {
            hit = true;
        }
    });
    hit
}

/// The annotation of the field `field` of the class instance `obj`
/// reads, when it is one.
pub(crate) fn class_field_ann(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    obj: ExprId,
    field: &str,
) -> Option<String> {
    let class = match expr_types.get(&obj)? {
        Type::ClassRef(c) => c,
        Type::Nullable(t) => match &**t {
            Type::ClassRef(c) => c,
            _ => return None,
        },
        _ => return None,
    };
    ast.stmts.iter().find_map(|s| match s {
        Stmt::TypeDecl { name, fields, .. } if name == class => (fields.iter())
            .find(|(k, _)| *k == field)
            .map(|(_, a)| a.clone()),
        _ => None,
    })
}
