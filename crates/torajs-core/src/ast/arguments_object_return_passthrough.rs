//! Pass-through returns of an `arguments` element, spelled so the
//! faces can take the body (rotation 626).
//!
//! `return cond ? arguments[1] : a` — the JavaScript default-argument
//! idiom — hands back an element box through a ternary arm, and the
//! return lowering's root retain cannot see through a ternary /
//! nullish / sequence / `as` / assignment arm
//! (`body_has_unsafe_return_arguments`). Every face therefore refused
//! such a body, which fell to the declared-params builder: `ret(5,
//! 6)` on `function ret(a) { return arguments.length > 1 ?
//! arguments[1] : a }` answered undefined, and the fn's value escape
//! died as "unknown function". Nothing about the shape is
//! unrewritable — only the return ROOT was: bound first,
//!
//! ```text
//! return E;   →   { const __torajs_retv: any = E; return __torajs_retv; }
//! ```
//!
//! the let-init lane owns the element read the way it owns any other
//! (`const before = arguments[1]` has always been fine), and the
//! return of a local is the moved-local return every fn already
//! takes. The rewrite runs first inside `desugar_arguments_object`,
//! so the scans that follow see a return root that is an identifier.

use super::arguments_object_walkers::body_has_unsafe_return_arguments;
use super::{Ast, Expr, Stmt};

pub(super) fn bind_passthrough_returns(ast: &mut Ast) {
    let mut stmts = std::mem::take(&mut ast.stmts);
    rewrite_list(ast, &mut stmts);
    ast.stmts = stmts;
}

fn rewrite_list(ast: &mut Ast, stmts: &mut [Stmt]) {
    for s in stmts.iter_mut() {
        rewrite_stmt(ast, s);
    }
}

fn rewrite_stmt(ast: &mut Ast, s: &mut Stmt) {
    match s {
        Stmt::Return(Some(e)) => {
            let e = *e;
            if !body_has_unsafe_return_arguments(ast, std::slice::from_ref(s)) {
                return;
            }
            let read = ast.add_expr(Expr::Ident("__torajs_retv".into()));
            *s = Stmt::Block(vec![
                Stmt::LetDecl {
                    mutable: false,
                    name: "__torajs_retv".into(),
                    type_ann: Some("any".into()),
                    init: e,
                    is_var: false,
                },
                Stmt::Return(Some(read)),
            ]);
        }
        Stmt::FnDecl { body, .. } | Stmt::Block(body) | Stmt::Multi(body) => {
            rewrite_list(ast, body)
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_stmt(ast, then_branch);
            if let Some(e) = else_branch {
                rewrite_stmt(ast, e);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => rewrite_stmt(ast, body),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                rewrite_stmt(ast, i);
            }
            rewrite_stmt(ast, body);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rewrite_list(ast, body);
            rewrite_list(ast, catch_body);
            if let Some(f) = finally_body {
                rewrite_list(ast, f);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                rewrite_list(ast, &mut c.body);
            }
            if let Some(d) = default {
                rewrite_list(ast, d);
            }
        }
        _ => {}
    }
}
