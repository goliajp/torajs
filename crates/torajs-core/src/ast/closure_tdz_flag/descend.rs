//! The statement lists nested in a statement, each flagged as a list
//! of its own.

use super::super::desugar_with::walk::arrow_nodes;
use super::super::switch_tdz_flag::flag_case_block;
use super::super::{Ast, Expr, ExprId, StaticInit, Stmt};
use super::{flag_list, wrap};

/// Every statement list nested in `s`: blocks, function and class
/// bodies, and the bodies of the closures its own expressions write.
pub(super) fn descend(ast: &mut Ast, s: &mut Stmt, counter: &mut usize) {
    for a in arrow_nodes(ast, s) {
        flag_arrow(ast, a, counter);
    }
    match s {
        Stmt::Block(v) => flag_list(ast, v, counter),
        Stmt::FnDecl { body: v, .. } => flag_list(ast, v, counter),
        Stmt::Multi(v) => {
            for c in v.iter_mut() {
                descend(ast, c, counter);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            flag_list(ast, body, counter);
            flag_list(ast, catch_body, counter);
            if let Some(f) = finally_body {
                flag_list(ast, f, counter);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            descend(ast, then_branch, counter);
            if let Some(e) = else_branch {
                descend(ast, e, counter);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => descend(ast, body, counter),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                descend(ast, i, counter);
            }
            descend(ast, body, counter);
        }
        Stmt::Switch {
            cases,
            default,
            default_at,
            ..
        } => {
            let heads = flag_case_block(ast, cases, default, *default_at, counter);
            for c in cases.iter_mut() {
                for b in c.body.iter_mut() {
                    descend(ast, b, counter);
                }
            }
            for b in default.iter_mut().flatten() {
                descend(ast, b, counter);
            }
            if !heads.is_empty() {
                wrap(s, |sw| heads.into_iter().chain([sw]).collect());
            }
        }
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => descend(ast, inner, counter),
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            if let Some(c) = ctor {
                flag_list(ast, &mut c.body, counter);
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                flag_list(ast, &mut m.body, counter);
            }
            for i in static_init.iter_mut() {
                match i {
                    StaticInit::Block(b) => flag_list(ast, b, counter),
                    StaticInit::Field(f) => {
                        let mut e = Stmt::Expr(f.init);
                        descend(ast, &mut e, counter);
                    }
                }
            }
        }
        _ => {}
    }
}

/// A closure's body is a list of its own; it is lifted out of the arena
/// for the walk and put back.
fn flag_arrow(ast: &mut Ast, eid: ExprId, counter: &mut usize) {
    let Expr::ArrowFn { body, .. } = &mut ast.exprs[eid.0 as usize] else {
        return;
    };
    let mut body = std::mem::take(body);
    flag_list(ast, &mut body, counter);
    if let Expr::ArrowFn { body: slot, .. } = &mut ast.exprs[eid.0 as usize] {
        *slot = body;
    }
}
