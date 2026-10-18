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

/// Whether the object literal `lit`, lowered against the annotation
/// `ann`, holds an `any` member that the annotation gives an object
/// type or an array of them, at any depth: the lowering casts that
/// member into its slot (`ssa_lower_object_lit::declared`). An array
/// literal lowered against an array annotation lays its object literal
/// elements out at the element type (`ssa_lower_array`), so it casts
/// when one of them does.
pub(crate) fn lit_casts_member(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    ann: &str,
    lit: ExprId,
) -> bool {
    if let Expr::Array(elems) = ast.get_expr(lit) {
        let Some(elem) = elem_ann(ann) else {
            return false;
        };
        return (elems.iter()).any(|e| lit_casts_member(ast, expr_types, &elem, *e));
    }
    let Expr::ObjectLit { fields } = ast.get_expr(lit) else {
        return false;
    };
    let owner = strip_nullable(ann.trim()).trim();
    let anns: Vec<(Vec<u8>, String)> =
        match crate::ast_collect_fn_closure_init::inlobj_fields(owner) {
            Some(f) => (f.into_iter())
                .map(|(k, _, a)| (k.into_bytes(), a))
                .collect(),
            None => match instance_fields(ast, owner) {
                Some(f) => (f.into_iter())
                    .map(|(k, a)| (k.as_wtf8().as_bytes().to_vec(), a))
                    .collect(),
                None => return false,
            },
        };
    fields.iter().any(|(k, v)| {
        let key = k.as_wtf8().as_bytes();
        let Some((_, a)) = anns.iter().find(|(n, _)| n == key) else {
            return false;
        };
        match expr_types.get(v) {
            Some(Type::Any) => cast_binding_ann(ast, a),
            _ => lit_casts_member(ast, expr_types, a, *v),
        }
    })
}

/// Whether the object literal `lit`, or an object literal element of
/// the array literal `lit`, holds an `any` member at any depth: the one
/// shape [`lit_casts_member`] can answer yes for.
fn lit_has_any_member(ast: &Ast, expr_types: &HashMap<ExprId, Type>, lit: ExprId) -> bool {
    if let Expr::Array(elems) = ast.get_expr(lit) {
        return (elems.iter()).any(|e| lit_has_any_member(ast, expr_types, *e));
    }
    let Expr::ObjectLit { fields } = ast.get_expr(lit) else {
        return false;
    };
    (fields.iter()).any(|(_, v)| {
        expr_types.get(v) == Some(&Type::Any) || lit_has_any_member(ast, expr_types, *v)
    })
}

/// Whether the object literal `lit` assigned to the binding `name`
/// holds a member [`lit_casts_member`] casts against the annotation
/// of that binding (`ssa_lower_assign_ident` pins the literal to it).
/// Every parameter and `let` of the name anywhere in the program is
/// read: a closure's write reaches the binding it captures.
pub(crate) fn lit_into_binding_casts(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    name: &str,
    lit: ExprId,
) -> bool {
    if !lit_has_any_member(ast, expr_types, lit) {
        return false;
    }
    let casts = |a: Option<&String>| a.is_some_and(|a| lit_casts_member(ast, expr_types, a, lit));
    let param = |ps: &[crate::ast::Param]| {
        (ps.iter()).any(|p| p.name == name && casts(p.type_ann.as_ref()))
    };
    let mut hit = false;
    crate::ast::walk_stmts(&ast.stmts, &mut |s| match s {
        Stmt::LetDecl {
            name: n, type_ann, ..
        } if n == name => hit |= casts(type_ann.as_ref()),
        Stmt::FnDecl { params, .. } => hit |= param(params),
        _ => {}
    });
    hit || (ast.exprs.iter()).any(|e| matches!(e, Expr::ArrowFn { params, .. } if param(params)))
}

/// Whether a call of the module function `callee` passes, as one of
/// `args`, an object literal with a member [`lit_casts_member`] casts
/// against the parameter's annotation (`ssa_lower_call_terminal` pins
/// the literal to it).
pub(crate) fn lit_arg_casts_member(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    callee: &str,
    args: &[ExprId],
) -> bool {
    if !(args.iter()).any(|a| lit_has_any_member(ast, expr_types, *a)) {
        return false;
    }
    let Some(params) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl { name, params, .. } if name == callee => Some(params),
        _ => None,
    }) else {
        return false;
    };
    (args.iter().zip(params)).any(|(a, p)| {
        (p.type_ann.as_deref()).is_some_and(|t| lit_casts_member(ast, expr_types, t, *a))
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

/// Whether the function `name`, declared at `span` returning `ret`,
/// returns an `any` value into an object return type it wrote, or an
/// object literal with a member [`lit_casts_member`] casts.
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
            && (expr_types.get(e) == Some(&Type::Any)
                || ret.is_some_and(|r| lit_casts_member(ast, expr_types, r, *e)))
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

/// The fields of the declared type `name` names, each with its
/// annotation as the type spells it: an instance of a generic
/// declaration (`W<string>`) writes its type arguments in place of the
/// parameters (`w: W<U>` reads `w: W<string>`). Where the arguments do
/// not line up with the parameters, [`decl_fields`]'.
pub(crate) fn instance_fields(ast: &Ast, name: &str) -> Option<Vec<(crate::ast::PropKey, String)>> {
    let name = name.trim();
    let args = name
        .split_once('<')
        .and_then(|(bare, rest)| Some((bare.trim(), rest.strip_suffix('>')?)));
    if let Some((bare, args)) = args {
        let args = split_type_args(args);
        let inst = ast.stmts.iter().find_map(|s| match s {
            Stmt::TypeDecl {
                name: n,
                type_params,
                fields,
            } if n == bare && type_params.len() == args.len() => Some((type_params, fields)),
            _ => None,
        });
        if let Some((params, fields)) = inst {
            let subst: Vec<(String, String)> = (params.iter().cloned())
                .zip(args.iter().map(|a| a.trim().to_string()))
                .collect();
            let spell = |a: &str| crate::ssa_lower_generics_monomorph::substitute_in_ann(a, &subst);
            return Some(
                (fields.iter())
                    .map(|(k, a)| (k.clone(), spell(a)))
                    .collect(),
            );
        }
    }
    decl_fields(ast, name).map(|(_, fields)| fields)
}

/// The type arguments of an argument list (`string, Map<K, V>`), split
/// at its top-level commas; the `>` of a function type's `->` closes
/// nothing.
fn split_type_args(args: &str) -> Vec<&str> {
    let (mut parts, mut depth, mut last, mut prev) = (Vec::new(), 0i32, 0usize, 0u8);
    for (i, &b) in args.as_bytes().iter().enumerate() {
        match b {
            b'>' if prev == b'-' => {}
            b'(' | b'<' | b'[' | b'{' => depth += 1,
            b')' | b'>' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(&args[last..i]);
                last = i + 1;
            }
            _ => {}
        }
        prev = b;
    }
    parts.push(&args[last..]);
    parts
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

    #[test]
    fn a_generic_instance_spells_its_fields_with_its_type_arguments() {
        let src = "type T = { x: number }; type W<U, V> = { t: T; u: U; v: Map<U, V> }; \
                   type N<U> = { w: W<U, (a: U) => U> };";
        let tokens = crate::lexer::tokenize(src).expect("tokenize");
        let ast = crate::parser::parse(src, &tokens).expect("parse");
        let anns = |ty: &str| -> Vec<(String, String)> {
            (instance_fields(&ast, ty).unwrap_or_default().into_iter())
                .map(|(k, a)| (k.lossy().to_string(), a))
                .collect()
        };
        let w = anns("W<string, Map<number, boolean>>");
        assert_eq!(w[0], ("t".into(), "T".into()));
        assert_eq!(w[1], ("u".into(), "string".into()));
        assert_eq!(w[2].0, "v");
        assert!(!names_type_param(&w[2].1, &["U".into(), "V".into()]));
        let n = anns("N<string>");
        assert_eq!(n.len(), 1);
        assert!(n[0].1.starts_with("W<string") && !n[0].1.contains('U'));
        // the arguments do not line up: the fields every instance shares
        assert_eq!(anns("W<string>"), [("t".to_string(), "T".to_string())]);
        assert_eq!(
            split_type_args("a, (x: b) -> c, M<d, e>"),
            ["a", " (x: b) -> c", " M<d, e>"]
        );
    }
}
