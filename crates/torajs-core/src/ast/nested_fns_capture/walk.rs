//! The descent half of the capture router — how the walk reaches each
//! statement list — split out of the parent under the file-size line.

use super::walk_list;
use crate::ast::{Ast, Stmt};

/// Descend into every statement list `s` owns. A `FnDecl` body is a
/// scope of its own and routes as one; the block-shaped statements carry
/// lists that route as blocks do.
pub(super) fn walk_children(
    ast: &mut Ast,
    s: &mut Stmt,
    globals: &[String],
    scope: &mut Vec<String>,
    in_block: bool,
    counter: &mut u32,
) {
    match s {
        Stmt::FnDecl { params, body, .. } => {
            let depth = scope.len();
            scope.extend(params.iter().map(|p| p.name.clone()));
            walk_list(ast, body, globals, scope, false, counter);
            scope.truncate(depth);
        }
        // A `Block` opens one; `Multi` is a compiler-minted sequence
        // sharing the surrounding scope, so it is whatever its parent is.
        Stmt::Block(b) => walk_list(ast, b, globals, scope, true, counter),
        Stmt::Multi(b) => walk_list(ast, b, globals, scope, in_block, counter),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_children(ast, then_branch, globals, scope, true, counter);
            if let Some(eb) = else_branch.as_deref_mut() {
                walk_children(ast, eb, globals, scope, true, counter);
            }
        }
        Stmt::ForOf { var_name, body, .. } => {
            let depth = scope.len();
            scope.push(var_name.clone());
            walk_children(ast, body, globals, scope, true, counter);
            scope.truncate(depth);
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::Labeled { body, .. } => walk_children(ast, body, globals, scope, true, counter),
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            walk_list(ast, body, globals, scope, true, counter);
            let depth = scope.len();
            if let Some(cp) = catch_param {
                scope.push(cp.clone());
            }
            walk_list(ast, catch_body, globals, scope, true, counter);
            scope.truncate(depth);
            if let Some(fb) = finally_body {
                walk_list(ast, fb, globals, scope, true, counter);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                walk_list(ast, &mut c.body, globals, scope, true, counter);
            }
            if let Some(d) = default {
                walk_list(ast, d, globals, scope, true, counter);
            }
        }
        _ => {}
    }
}
