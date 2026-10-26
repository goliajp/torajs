//! The one-level statement walk the param-tag passes share.

use crate::ast::Stmt;

/// Push every Stmt nested inside `s` (one level) onto the walk stack.
pub(crate) fn push_child_stmts<'a>(s: &'a Stmt, stack: &mut Vec<&'a Stmt>) {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stack.push(then_branch);
            if let Some(e) = else_branch {
                stack.push(e);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => stack.push(body),
        Stmt::Labeled { body, .. } => stack.push(body),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                stack.push(i);
            }
            stack.push(body);
        }
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => stack.push(body),
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                stack.extend(c.body.iter());
            }
            if let Some(d) = default {
                stack.extend(d.iter());
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            stack.extend(body.iter());
            stack.extend(catch_body.iter());
            if let Some(f) = finally_body {
                stack.extend(f.iter());
            }
        }
        Stmt::Block(inner) | Stmt::Multi(inner) => stack.extend(inner.iter()),
        Stmt::FnDecl { body, .. } => stack.extend(body.iter()),
        Stmt::ExportDecl { inner: Some(i), .. } => {
            stack.push(i);
        }
        _ => {}
    }
}
