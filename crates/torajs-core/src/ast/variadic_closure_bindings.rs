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

use super::{Ast, Expr, ExprId, Stmt};

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
            _ => {
                crate::ast_refs::closure_alias_fn_canon(ast, *init)
                    .is_some_and(|c| c.contains("__rest("))
                    || var_rooted_alias_is_variadic(ast, *init)
                    || field_read_is_variadic(ast, *init)
            }
        };
        if is_variadic {
            out.push(name.clone());
        }
    }
    ast.variadic_value_bindings.extend(out);
}

/// A read of a variadic closure FIELD (`const g = src.f`), through
/// the same spelling that types the binding's slot. That spelling is
/// asked without the checker's degrade / widen sets, which do not
/// exist yet: a binding they move to `any` never resolves to a
/// Closure slot, and only a Closure-typed callee consults this record,
/// so the extra name is never read.
fn field_read_is_variadic(ast: &Ast, init: ExprId) -> bool {
    let none = std::collections::HashSet::new();
    crate::ast_refs::closure_field_read_ann(ast, init, &none, &none)
        .is_some_and(|c| c.contains("__rest("))
}

fn fn_decl_is_variadic(ast: &Ast, fn_name: &str) -> bool {
    ast.stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name, params, .. }
            if name == fn_name && params.last().is_some_and(|p| p.is_rest))
    })
}

/// The same alias question for a chain whose ROOT is a `var`
/// (`var gv = (...xs: number[]) => …; const gb = gv`).
///
/// `closure_alias_fn_canon` cannot answer it and must not learn to:
/// that spelling is what TYPES the two slots, and every promote arm
/// keeps `var` out because a `var`'s hoisted home is not the slot
/// being typed. This record asks a different question — which route
/// a CALL takes once the reader is not the declaring frame — and the
/// module doc already admits `var` on exactly that ground for the
/// direct flavor. Answering it here keeps the two questions apart
/// rather than widening the typing spelling to reach it.
///
/// Over-recording is the safe direction here and under-recording is
/// not: the boxed dual entry serves a closure of any declared arity
/// (r601-02 — a fixed-arity closure behind a variadic face rides it
/// correctly), while a missed record leaves the call on a static
/// dispatch against the fixed-prefix sig — arity 0 for a bare rest,
/// which segfaults. So a name declared `var` more than once counts
/// if ANY of them is variadic.
///
/// Runs BEFORE `desugar_var_hoist` (the pipeline puts
/// `synthesize_forwarders` at the pass that calls this, the hoist
/// six passes later), so a `var` declaration still carries its own
/// initializer here; after the hoist it would be a bare `Uninit`
/// with the closure moved into a separate assignment.
fn var_rooted_alias_is_variadic(ast: &Ast, init: ExprId) -> bool {
    let Expr::Ident(src) = ast.get_expr(init) else {
        return false;
    };
    // Walk the `let` / `const` hops the same way the canon does —
    // one at a time, each hop a uniquely-declared name, a name
    // reached twice being a cycle — and ask the `var` question at
    // whatever name the chain bottoms out on.
    let mut name: &str = src;
    let mut seen: Vec<&str> = Vec::new();
    loop {
        if seen.contains(&name) {
            return false;
        }
        seen.push(name);
        if toplevel_var_decls_are_variadic(ast, name) {
            return true;
        }
        let mut next: Option<&str> = None;
        for s in &ast.stmts {
            let Stmt::LetDecl {
                name: decl,
                init,
                is_var: false,
                ..
            } = s
            else {
                continue;
            };
            if decl != name {
                continue;
            }
            // A second declaration means the name at the alias site
            // is not necessarily the one found here.
            if next.is_some() {
                return false;
            }
            match ast.get_expr(*init) {
                Expr::Ident(n) => next = Some(n),
                _ => return false,
            }
        }
        let Some(n) = next else {
            return false;
        };
        name = n;
    }
}

/// Does any top-level `var` declaring `name` hold a variadic closure
/// — by the closure's own rest parameter, or by a variadic face
/// written on the declaration? Both halves are the same two the
/// direct flavor above reads.
fn toplevel_var_decls_are_variadic(ast: &Ast, name: &str) -> bool {
    ast.stmts.iter().any(|s| {
        let Stmt::LetDecl {
            name: decl,
            init,
            type_ann,
            is_var: true,
            ..
        } = s
        else {
            return false;
        };
        decl == name
            && (type_ann.as_deref().is_some_and(|a| a.contains("__rest("))
                || matches!(ast.get_expr(*init),
                    Expr::Closure { fn_name, .. } if fn_decl_is_variadic(ast, fn_name)))
    })
}
