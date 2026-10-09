//! Top-level statement flattening (rotation 230, RFC
//! 20260727-dstr-assignment 刀 6) — the shared view both K.3
//! global-registration walks iterate.

use super::{Ast, Stmt};

/// Top-level statements with `Stmt::Multi` groups flattened.
/// Multi-declarator lets (`let a, b;`) and parse-time desugars
/// (destructuring) wrap their LetDecls in `Multi`, which shares the
/// surrounding scope — so any walk that registers top-level bindings
/// (the K.3 data-global pre-pass in check_pipeline and
/// `collect_toplevel_globals`) must see through it, or a
/// multi-declarator binding never becomes a global and a named-fn
/// write answers "assignment to undeclared" (rotation 230: the
/// test262 dstr-assignment preamble is exactly `let v2, vNull, …;`
/// plus writes inside an async fn).
pub fn toplevel_stmts_flat(ast: &Ast) -> Vec<&Stmt> {
    stmts_flat(&ast.stmts)
}

/// Some top-level declaration binds `name` — a `let` / `const` /
/// `var`, a `using`, a function or a class, exported or not. That is
/// the global environment's declarative half (§9.1.1.4): from the
/// first statement on, a bare `name` reaches it rather than the
/// global object, uninitialized (§8.1.1.1) until its declaration runs.
pub fn toplevel_binds(ast: &Ast, name: &str) -> bool {
    toplevel_stmts_flat(ast).into_iter().any(|s| {
        let s = match s {
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => inner.as_ref(),
            s => s,
        };
        matches!(
            s,
            Stmt::LetDecl { name: n, .. }
            | Stmt::UsingDecl { name: n, .. }
            | Stmt::FnDecl { name: n, .. }
            | Stmt::ClassDecl { name: n, .. } if n == name
        )
    })
}

/// Any statement list with its `Stmt::Multi` groups flattened — the
/// same view for a list that is not the top level (a switch's clause
/// bodies share one scope the same way).
pub fn stmts_flat(stmts: &[Stmt]) -> Vec<&Stmt> {
    fn walk<'a>(stmts: &'a [Stmt], out: &mut Vec<&'a Stmt>) {
        for s in stmts {
            match s {
                Stmt::Multi(inner) => walk(inner, out),
                other => out.push(other),
            }
        }
    }
    let mut out = Vec::new();
    walk(stmts, &mut out);
    out
}
