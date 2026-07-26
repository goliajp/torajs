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
    fn walk<'a>(stmts: &'a [Stmt], out: &mut Vec<&'a Stmt>) {
        for s in stmts {
            match s {
                Stmt::Multi(inner) => walk(inner, out),
                other => out.push(other),
            }
        }
    }
    let mut out = Vec::new();
    walk(&ast.stmts, &mut out);
    out
}
