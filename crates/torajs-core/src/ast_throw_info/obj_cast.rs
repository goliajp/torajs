//! The boundaries where an `any` value crosses into an object-layout
//! slot the program annotated raise a TypeError when the value is not
//! a cell the slot can read (`ssa_lower_checked_cast::obj`): a `let`
//! initializer, an assignment, a return. The lowering decides each
//! one from the slot's annotation; this answers the same question
//! from the source, erring towards "may raise".

use crate::ast::{Ast, Expr, ExprId, Stmt};
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
    let class = class_of(expr_types, obj)?;
    ast.stmts.iter().find_map(|s| match s {
        Stmt::TypeDecl { name, fields, .. } if *name == class => (fields.iter())
            .find(|(k, _)| *k == field)
            .map(|(_, a)| a.clone()),
        _ => None,
    })
}

/// The class whose instance `e` reads, by its checked type.
pub(crate) fn class_of(expr_types: &HashMap<ExprId, Type>, e: ExprId) -> Option<String> {
    match expr_types.get(&e)? {
        Type::ClassRef(c) => Some(c.clone()),
        Type::Nullable(t) => match &**t {
            Type::ClassRef(c) => Some(c.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// The annotation the program wrote for the value `e` reads, traced
/// through a binding (`binding` answers a name's written annotation)
/// and the fields of class instances (`class_of` names the class an
/// expression's checked type is) and of values whose own annotation is
/// traced the same way. `None` for any other expression, and wherever
/// a step is the compiler's inference. The checker and the lowering
/// both read it, so a push the checker admits on it is one the
/// lowering casts.
pub(crate) fn written_ann(
    ast: &Ast,
    e: ExprId,
    binding: &dyn Fn(&str) -> Option<String>,
    class_of: &dyn Fn(ExprId) -> Option<String>,
) -> Option<String> {
    match ast.get_expr(e) {
        Expr::Ident(n) => binding(n),
        Expr::Member { obj, name } => {
            let owner = class_of(*obj).or_else(|| written_ann(ast, *obj, binding, class_of))?;
            field_ann(ast, strip_nullable(&owner).trim(), name)
        }
        _ => None,
    }
}

/// The annotation of the field `field` of the object type `owner`: an
/// object type literal's, or a declared non-generic type's (a class's
/// among them).
fn field_ann(ast: &Ast, owner: &str, field: &str) -> Option<String> {
    if let Some(fields) = crate::ast_collect_fn_closure_init::parse_inlobj_field_anns(owner) {
        return fields.get(field).cloned();
    }
    ast.stmts.iter().find_map(|s| match s {
        Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } if name == owner && type_params.is_empty() => (fields.iter())
            .find(|(k, _)| *k == field)
            .map(|(_, a)| a.clone()),
        _ => None,
    })
}

/// The element annotation of the array annotation `ann` (`T[]` or
/// `Array<T>`, either one `| undefined`).
pub(crate) fn elem_ann(ann: &str) -> Option<String> {
    let ann = strip_nullable(ann).trim();
    let elem = match ann.strip_suffix("[]") {
        Some(e) => e,
        None => ann.strip_prefix("Array<")?.strip_suffix('>')?,
    };
    Some(elem.trim().to_string())
}
