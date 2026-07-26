//! A function that mentions `this` takes `__this` as a parameter.
//!
//! RFC 20260726-new-on-function blade 1. `desugar_classes` rewrites
//! every `Expr::This` into `Ident("__this")` unconditionally
//! (`desugar_classes_pass2.rs`), but only class members end up with a
//! binding for that name: `__cm_<C>__ctor` and every method receive
//! `__this` as their first *parameter*, and the factory
//! `build_factory_body` mints passes the fresh instance into it.
//!
//! A plain function gets no such parameter, so the checker rejects it
//! with `unknown identifier __this` — and it does so at *declaration*
//! time, whether or not the function is ever used as a constructor.
//! `function F(x: number) { this.x = x }` alone is enough; that is the
//! 783-case cluster spanning 55 test262 directories, the widest spread
//! in the census.
//!
//! The test is therefore exact rather than heuristic: **a function whose
//! body has `__this` free**. Class members never qualify (the name is
//! bound by their parameter list), plain functions always do, and no
//! name-matching on `__cm_`/`__new_` prefixes is needed.
//!
//! `this` is bound to `undefined` at a plain call. TypeScript is strict
//! by default and spec §10.2.1.1 skips the ToObject coercion in strict
//! mode, so `F(1)` sees `undefined` and `this.x` throws a TypeError —
//! matching bun. Binding it to a fresh object is blade 2's job, and only
//! for `new F(1)`.

use super::free_vars::free_vars_of_body;
use super::{Ast, Expr, Param, Stmt};

/// Names of the top-level functions that gained a `__this` parameter.
/// Call sites need it too, and a function may be called before it is
/// declared, so collection and rewriting are separate walks.
fn functions_needing_this(ast: &Ast) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for st in &ast.stmts {
        let Stmt::FnDecl {
            name, params, body, ..
        } = st
        else {
            continue;
        };
        let bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
        if free_vars_of_body(ast, &bound, body)
            .iter()
            .any(|v| v == "__this")
        {
            out.push(name.clone());
        }
    }
    out
}

pub fn bind_this_param(ast: &mut Ast) {
    let needs = functions_needing_this(ast);
    if needs.is_empty() {
        return;
    }

    for st in &mut ast.stmts {
        let Stmt::FnDecl { name, params, .. } = st else {
            continue;
        };
        if needs.iter().any(|n| n == name) {
            params.insert(
                0,
                Param {
                    name: "__this".into(),
                    // Not the declaring class's nominal type, the way a
                    // class factory annotates it: a plain function's
                    // field set is whatever its branches assign, so the
                    // receiver is a dynamic object.
                    type_ann: Some("any".into()),
                    default: None,
                    is_rest: false,
                },
            );
        }
    }

    // Direct calls only — the callee has to be a name this pass rewrote.
    // A call through a value (`const g = F; g(1)`) cannot be reached from
    // here and is left alone deliberately: RFC §4 records it as a known
    // boundary for blade 4 rather than papering over it with a silently
    // wrong arity.
    let sites: Vec<usize> = (0..ast.exprs.len())
        .filter(|&i| match &ast.exprs[i] {
            Expr::Call { callee, .. } => match &ast.exprs[callee.0 as usize] {
                Expr::Ident(n) => needs.iter().any(|x| x == n),
                _ => false,
            },
            _ => false,
        })
        .collect();
    for i in sites {
        let undef = ast.add_expr(Expr::Ident("undefined".into()));
        if let Expr::Call { args, .. } = &mut ast.exprs[i] {
            args.insert(0, undef);
        }
    }
}
