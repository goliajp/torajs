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

/// Record every top-level `let` / `const` holding a lifted closure
/// that is variadic on EITHER side: the closure's own last parameter
/// is a rest, or the binding's annotation says the value is called
/// variadically. The second half is not the same fact as the first —
/// `const t: (...xs: number[]) => number = (a: number) => a * 2` is a
/// one-parameter closure reached through a variadic face, and its
/// calls have to box exactly like the other flavor's, because it is
/// the CALL SITE that has any arity. Recording only the first half
/// promoted such a binding into a static dispatch and `t(21)`
/// answered 4 (variadic-fn-type-ann-002 / -003).
///
/// An alias of either flavor (`const gb = ga`) is recorded too: the
/// alias holds the same env cell, and a named-fn body reading it
/// dispatches through the same route. Without the record the alias's
/// call took a static dispatch on the fixed-prefix sig — arity 0 for
/// a bare rest — and segfaulted.
///
/// `var` is INCLUDED, unlike every promote arm: this record is not a
/// promotion, it is the routing a call takes once the reader is not
/// the declaring frame, and a `var` binding read from a named fn body
/// is exactly that reader. Leaving it out let such a call take a
/// static dispatch and segfault.
pub(crate) fn record(ast: &mut Ast) {
    let mut out: Vec<String> = Vec::new();
    for s in &ast.stmts {
        let Stmt::LetDecl {
            name,
            init,
            type_ann,
            ..
        } = s
        else {
            continue;
        };
        let ann_is_variadic = type_ann.as_deref().is_some_and(|a| a.contains("__rest("));
        let is_variadic = match ast.get_expr(*init) {
            Expr::Closure { fn_name, .. } => ann_is_variadic || fn_decl_is_variadic(ast, fn_name),
            // An ALIAS of such a binding (`const gb = ga`) holds the
            // identical env cell, so it is called the identical way.
            // Asked through the same one-hop spelling the alias's
            // slot type is decided by, so the two answers cannot
            // disagree: if the canon that types the slot says rest,
            // the route that calls it says rest.
            _ => crate::ast_refs::closure_alias_fn_canon(ast, *init)
                .is_some_and(|c| c.contains("__rest(")),
        };
        if is_variadic {
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
