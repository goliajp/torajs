//! Which top-level bindings hold a VARIADIC lifted closure.
//!
//! A `const ga = (...xs: number[]): number => …` promotes to a global
//! slot like any other closure binding, and its calls have to route
//! the boxed dual entry: one binding serves closures of any declared
//! arity, so there is no static sig to dispatch on. Inside the frame
//! that declares it the let lane records that in `variadic_locals`;
//! a promoted binding has no let lowering in the fn that reads it,
//! which is why the promoted flavor needs a name record of its own
//! (`Ast::variadic_value_bindings`, shared with the forwarder-wrap
//! flavor `collect_let_init_axis_rewrites` fills).
//!
//! Name-keyed, and read behind a locals miss: a fn-local binding of
//! the same name shadows the global and keeps its own route.

use super::{Ast, Expr, Stmt};

/// Record every top-level `let` / `const` whose init is a lifted
/// closure whose last user parameter is a rest. `var` is excluded for
/// the reason every promote arm excludes it — its hoisted home is not
/// this slot.
pub(crate) fn record(ast: &mut Ast) {
    let mut out: Vec<String> = Vec::new();
    for s in &ast.stmts {
        let Stmt::LetDecl {
            name,
            init,
            is_var: false,
            ..
        } = s
        else {
            continue;
        };
        let Expr::Closure { fn_name, .. } = ast.get_expr(*init) else {
            continue;
        };
        if fn_decl_is_variadic(ast, fn_name) {
            out.push(name.clone());
        }
    }
    ast.variadic_value_bindings.extend(out);
}

fn fn_decl_is_variadic(ast: &Ast, fn_name: &str) -> bool {
    ast.stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name, params, .. }
            if name == fn_name && params.last().is_some_and(|p| p.is_rest))
    })
}
