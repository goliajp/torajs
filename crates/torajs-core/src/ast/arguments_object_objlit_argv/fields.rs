//! The object-literal fields the boxed-only admission considers, and
//! the accessor test the arrow-arguments alias pass shares with it.

use std::collections::HashMap;

use super::super::expr::Param;
use super::super::{Ast, Expr, ExprId, PropKey, Stmt};

/// Is `value`, the value of field `key` of an object literal, a getter
/// or setter whose `arguments` the admission takes? Such an accessor is
/// a method definition (§15.4.4) with an arguments object of its own:
/// [[Get]] calls it with none, [[Set]] with the one value (§10.1.8.1,
/// §10.1.9.2). Its slot (`__getter_<p>` / `__setter_<p>`, or a computed
/// one the dynobj define kernel installs) is never spelled by a member
/// reference, so every call reaches it through the accessor lanes or
/// the closure cell's boxed adapter, which feed the real count.
///
/// The alias pass asks this before the lift (`params` are the arrow's),
/// the admission after it (`params` lead with `__env`), and both must
/// answer alike: an accessor the alias pass gives its own scope but the
/// admission refuses has no `arguments` at all. One outside this test
/// keeps the enclosing function's, as every arrow does.
pub(in crate::ast) fn accessor_owns_arguments(
    ast: &Ast,
    key: &PropKey,
    value: ExprId,
    params: &[Param],
) -> bool {
    let accessor = ast.objlit_computed_accessors.contains_key(&value)
        || ast.objlit_method_exprs.contains(&value)
            && key
                .as_str()
                .is_some_and(|k| k.starts_with("__getter_") || k.starts_with("__setter_"));
    // The admission's param budget (`admit_boxed_only`), counted
    // without the env slot the lift adds.
    let user: Vec<&Param> = params.iter().filter(|p| p.name != "__env").collect();
    accessor && user.len() < 5 && user.iter().all(|p| p.default.is_none() && !p.is_rest)
}

/// Before the lift: does a literal hold an accessor that reads its own
/// `arguments`? Its value then types as the argv face's rest tail.
pub(in crate::ast) fn holds_arguments_accessor(ast: &Ast, fields: &[(PropKey, ExprId)]) -> bool {
    fields.iter().any(|(k, e)| {
        matches!(ast.get_expr(*e), Expr::ArrowFn { params, body, .. }
            if accessor_owns_arguments(ast, k, *e, params)
                && super::super::arguments_object_walkers::body_has_any_arguments_touch(ast, body))
    })
}

/// (key, field) → lifted closure fn name: every closure field of a
/// top-level `let`, keyed by the binding, and every accessor anywhere
/// in the arena that passes [`accessor_owns_arguments`], keyed by its
/// closure — wherever its literal stands (a binding's init, a return,
/// a call argument, an assignment, a nested literal, an array element).
pub(super) fn objlit_fields(ast: &Ast) -> HashMap<(String, String), String> {
    let mut out: HashMap<(String, String), String> = HashMap::new();
    for s in &ast.stmts {
        let Stmt::LetDecl { name, init, .. } = s else {
            continue;
        };
        let Expr::ObjectLit { fields } = ast.get_expr(*init) else {
            continue;
        };
        for (fname, feid) in fields
            .iter()
            .filter_map(|(f, e)| f.as_str().map(|f| (f, e)))
        {
            if let Expr::Closure { fn_name, .. } = ast.get_expr(*feid) {
                out.insert((name.clone(), fname.to_string()), fn_name.clone());
            }
        }
    }
    for e in &ast.exprs {
        let Expr::ObjectLit { fields } = e else {
            continue;
        };
        for (key, feid) in fields {
            let (Some(fname), Expr::Closure { fn_name, .. }) = (key.as_str(), ast.get_expr(*feid))
            else {
                continue;
            };
            if lifted_params(ast, fn_name)
                .is_some_and(|ps| accessor_owns_arguments(ast, key, *feid, ps))
            {
                out.insert((fn_name.clone(), fname.to_string()), fn_name.clone());
            }
        }
    }
    out
}

fn lifted_params<'a>(ast: &'a Ast, fn_name: &str) -> Option<&'a [Param]> {
    ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl { name, params, .. } if name == fn_name => Some(params.as_slice()),
        _ => None,
    })
}
