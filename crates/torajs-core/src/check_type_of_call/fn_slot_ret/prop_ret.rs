//! The property, element and return slots of the census
//! (`ast::face_slots`): `o.f()` through a field some store handed a
//! function value of another face, `hs[i]()` through such an element,
//! and `ret()()` through what such a function returns.
//!
//! The stores are found by scanning the whole arena, as the binding
//! lane does, so the verdict does not depend on which store was checked
//! first: every value written to a field of that name (an assignment or
//! an object literal's field, of any receiver), or every `return` of the
//! function. A store of another receiver only makes the verdict more
//! careful.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::Checker;
use crate::check::face_slots::{self, field_read, ret_read};

/// A slot's census key, and its stores paired with the slot's spelling.
type Slot = (String, Option<Vec<(String, ExprId)>>);

/// The census key of the slot `callee` reads, and the stores it holds
/// paired with the slot's spelling; `None` stores when the slot's type
/// is not on record.
pub(super) fn slot(checker: &mut Checker, ast: &Ast, callee: ExprId) -> Option<Slot> {
    // the slot's spelling only once the census holds it: every call
    // through a field asks
    let pair = |checker: &Checker, stores: Vec<ExprId>| {
        let ann = crate::check::type_to_ann(checker.expr_types.get(&callee)?);
        Some(stores.into_iter().map(|e| (ann.clone(), e)).collect())
    };
    // an element's stores are not traced: an array moves between
    // bindings, and a store through any of them reaches the element
    if face_slots::elem_source(ast, callee).is_some() {
        return Some((
            format!("elem:{}", face_slots::elem_value(checker, ast, callee)?),
            None,
        ));
    }
    match ast.get_expr(callee) {
        Expr::Member { obj, name } => {
            let owner = field_read(checker, ast, *obj, name)?;
            Some((
                format!("field:{owner}:{name}"),
                pair(checker, field_stores(ast, name)),
            ))
        }
        Expr::Call { callee: f, .. } => {
            let Expr::Ident(g) = ast.get_expr(*f) else {
                return None;
            };
            if !ret_read(checker, g) {
                return None;
            }
            Some((format!("ret:{g}"), pair(checker, return_stores(ast, g))))
        }
        _ => None,
    }
}

fn field_stores(ast: &Ast, name: &str) -> Vec<ExprId> {
    let names_field =
        |t: ExprId| matches!(ast.get_expr(t), Expr::Member { name: n, .. } if n == name);
    let mut out = Vec::new();
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, value } if names_field(*target) => out.push(*value),
            Expr::ObjectLit { fields } => {
                out.extend(fields.iter().filter(|(k, _)| *k == *name).map(|(_, v)| *v));
            }
            _ => {}
        }
    }
    out
}

/// Every `return` of function `g`, or of the closure a `const g` holds.
fn return_stores(ast: &Ast, g: &str) -> Vec<ExprId> {
    fn walk(stmts: &[Stmt], out: &mut Vec<ExprId>) {
        for s in stmts {
            if let Stmt::Return(Some(e)) = s {
                out.push(*e);
            }
            crate::ast::stmt_nested_lists::for_each_nested_list(s, &mut |inner| walk(inner, out));
        }
    }
    fn closures<'a>(ast: &'a Ast, stmts: &'a [Stmt], g: &str, out: &mut Vec<&'a str>) {
        for s in stmts {
            if let Stmt::LetDecl {
                name,
                init,
                mutable: false,
                ..
            } = s
                && name == g
                && let Expr::Closure { fn_name, .. } = ast.get_expr(*init)
            {
                out.push(fn_name);
            }
            crate::ast::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
                closures(ast, inner, g, out)
            });
        }
    }
    let mut fns = vec![g];
    closures(ast, &ast.stmts, g, &mut fns);
    let mut out = Vec::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, body, .. } = s
            && fns.contains(&name.as_str())
        {
            walk(body, &mut out);
        }
    }
    out
}
