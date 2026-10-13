//! A function declaration of an async or generator body turned into a
//! value binding (`select`'s `Route::Value`).

use super::super::{Ast, Expr, Stmt};

/// `function f(p): R { … }` → `const f = function (p): R { … }`; an
/// async declaration's value is marked async. A function declaration's
/// binding is mutable (§10.2.11 step 36 creates it with
/// `CreateMutableBinding`), so when the scope `assigned` the name it
/// is a `let` of any value. When a written `var` shares the binding
/// (`var_shared`) it is a `var` of any value, which the var hoist
/// merges with the user's into one slot.
pub(super) fn to_value_binding(ast: &mut Ast, s: Stmt, assigned: bool, var_shared: bool) -> Stmt {
    let Stmt::FnDecl {
        name,
        params,
        return_type,
        body,
        span,
        is_generator,
        ..
    } = s
    else {
        unreachable!("caller checked");
    };
    let eid = ast.add_expr(Expr::ArrowFn {
        params,
        return_type,
        body,
    });
    ast.set_expr_span(eid, span);
    if is_generator {
        // An async generator expression, the parser's own marks.
        ast.gen_fn_exprs.insert(
            eid,
            super::super::GenFnExprInfo {
                kind: super::super::GenFnExprKind::AsyncGenerator,
                destr_prefix: 0,
            },
        );
    } else {
        // §15.2.5 InstantiateOrdinaryFunctionObject names the function
        // after its declaration; the value takes the NamedEvaluation
        // slot a destructuring default's anonymous definition takes
        let shown = ast.renamed_binding_display.get(&name).unwrap_or(&name);
        ast.dstr_default_names.insert(eid, shown.clone());
        ast.fn_expr_exprs.insert(eid);
        if ast.async_fns.contains(&name) {
            ast.async_fn_value_exprs.insert(eid);
        }
    }
    let writable = assigned || var_shared;
    Stmt::LetDecl {
        mutable: writable,
        name,
        type_ann: writable.then(|| "any".to_string()),
        init: eid,
        is_var: var_shared,
    }
}
