//! The boundaries where an `any` value crosses into an object-layout
//! slot the program annotated raise a TypeError when the value is not
//! a cell the slot can read (`ssa_lower_checked_cast::obj`): a `let`
//! initializer, an assignment, a return. The lowering decides each
//! one from the slot's annotation; this answers the same question
//! from the source, erring towards "may raise".

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::Type;
use crate::check_type_ann::strip_nullable;
use crate::lexer::Span;
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

/// Whether a binding annotated `ann` is a slot the object cast guards
/// (`ssa_lower_checked_cast::obj`): an object type, a written object
/// type literal, or an array of either at any depth.
pub(crate) fn cast_binding_ann(ast: &Ast, ann: &str) -> bool {
    let base = strip_nullable(ann).trim();
    object_ann(ast, base)
        || (base.starts_with("__inlobj(") && base != "__inlobj()")
        || elem_ann(base).is_some_and(|e| cast_binding_ann(ast, &e))
}

/// Whether a binding of checker type `t` is an object layout.
pub(crate) fn object_ty(t: Option<&Type>) -> bool {
    match t {
        Some(Type::Nullable(t)) => object_ty(Some(t)),
        Some(Type::Struct(_) | Type::ClassRef(_)) => true,
        _ => false,
    }
}

/// Whether the function `name`, declared at `span` returning `ret`,
/// returns an `any` value into an object return type it wrote.
pub(crate) fn returns_any_into_obj(
    ast: &Ast,
    name: &str,
    ret: Option<&str>,
    span: Span,
    body: &[Stmt],
    expr_types: &HashMap<ExprId, Type>,
) -> bool {
    let written_shape = |r: &str| {
        let base = strip_nullable(r).trim();
        base.starts_with("__inlobj(") && base != "__inlobj()" && ast.fn_ret_written(name, span)
    };
    if !ret.is_some_and(|r| object_ann(ast, r) || written_shape(r))
        || ast.contextual_ret_fns.contains(name)
    {
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

/// [`class_field_ann`], an instance of a generic class reading its
/// declaration's field ([`decl_fields`]).
pub(crate) fn instance_field_ann(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    obj: ExprId,
    field: &str,
) -> Option<String> {
    let class = class_of(expr_types, obj)?;
    let (_, fields) = decl_fields(ast, &class)?;
    fields
        .into_iter()
        .find(|(k, _)| *k == field)
        .map(|(_, a)| a)
}

/// The declaration the type `name` names and its fields, each with its
/// annotation. An instance of a generic declaration (`W<number>`) reads
/// its declaration's fields, less the ones whose annotation names a
/// type parameter: those do not spell the instance's type.
pub(crate) fn decl_fields<'a>(
    ast: &'a Ast,
    name: &str,
) -> Option<(&'a str, Vec<(crate::ast::PropKey, String)>)> {
    let bare = name.split('<').next().unwrap_or(name);
    ast.stmts.iter().find_map(|s| match s {
        Stmt::TypeDecl {
            name: n,
            type_params,
            fields,
        } if n == name || (n == bare && !type_params.is_empty()) => {
            let plain = fields
                .iter()
                .filter(|(_, a)| !names_type_param(a, type_params));
            Some((n.as_str(), plain.cloned().collect()))
        }
        _ => None,
    })
}

/// Whether the annotation `ann` spells one of `params`.
pub(crate) fn names_type_param(ann: &str, params: &[String]) -> bool {
    !params.is_empty()
        && ann
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
            .any(|w| params.iter().any(|p| p == w))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_type_parameter_is_matched_as_a_whole_name() {
        let t = ["T".to_string()];
        assert!(names_type_param("__nullable(T)", &t));
        assert!(names_type_param("Map<string, T>", &t));
        assert!(names_type_param("T[]", &t));
        assert!(!names_type_param("Tree | null", &t));
        assert!(!names_type_param("__nullable(Inner)", &t));
        assert!(!names_type_param("T", &[]));
    }

    #[test]
    fn a_generic_instance_reads_the_fields_every_instance_lays_out_alike() {
        let src = "type Inner = { b: number }; type W<T> = { inner: Inner | null; t: T | null };";
        let tokens = crate::lexer::tokenize(src).expect("tokenize");
        let ast = crate::parser::parse(src, &tokens).expect("parse");
        let names = |ty: &str| -> Vec<String> {
            decl_fields(&ast, ty)
                .map(|(_, fs)| fs.iter().map(|(k, _)| k.lossy().to_string()).collect())
                .unwrap_or_default()
        };
        assert_eq!(names("W<number>"), ["inner"]);
        assert_eq!(names("Inner"), ["b"]);
        assert!(names("V<number>").is_empty());
    }
}
