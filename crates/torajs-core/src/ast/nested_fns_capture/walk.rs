//! The descent half of the capture router — how the walk reaches each
//! statement list — split out of the parent under the file-size line.

use std::collections::HashMap;

use super::walk_list;
use crate::ast::scope_chain::{arrow_frames, fn_names, for_init_names};
use crate::ast::{Ast, ExprId, Stmt};

/// The bindings the enclosing scopes introduce, kept two ways.
#[derive(Default)]
pub(super) struct Scopes {
    /// What §B.3.3's block-binding split asks about: the lexical
    /// declarations seen so far, parameters, catch and for-of names.
    pub(super) annexb: Vec<String>,
    /// Every name a frame between the list and the top level binds
    /// (`scope_chain`) — whether a top-level fn name is rebound.
    pub(super) shadow: Vec<String>,
}

impl Scopes {
    /// The scopes an arrow's body list sits inside.
    pub(super) fn of_arrow(
        ast: &Ast,
        chains: &HashMap<ExprId, Vec<String>>,
        eid: ExprId,
        body: &[Stmt],
    ) -> Self {
        Self {
            annexb: Vec::new(),
            shadow: arrow_frames(ast, chains.get(&eid), eid, body),
        }
    }

    pub(super) fn mark(&self) -> (usize, usize) {
        (self.annexb.len(), self.shadow.len())
    }

    pub(super) fn reset(&mut self, (a, s): (usize, usize)) {
        self.annexb.truncate(a);
        self.shadow.truncate(s);
    }
}

/// Descend into every statement list `s` owns. A `FnDecl` body is a
/// scope of its own and routes as one; the block-shaped statements carry
/// lists that route as blocks do.
pub(super) fn walk_children(
    ast: &mut Ast,
    s: &mut Stmt,
    globals: &[String],
    scope: &mut Scopes,
    in_block: bool,
    counter: &mut u32,
) {
    let depth = scope.mark();
    match s {
        Stmt::FnDecl { params, body, .. } => {
            scope.annexb.extend(params.iter().map(|p| p.name.clone()));
            fn_names(params, body, &mut scope.shadow);
            walk_list(ast, body, globals, scope, false, counter);
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
            scope.annexb.push(var_name.clone());
            scope.shadow.push(var_name.clone());
            walk_children(ast, body, globals, scope, true, counter);
        }
        Stmt::ForOfSplitIter { var_name, body, .. } => {
            scope.shadow.push(var_name.clone());
            walk_children(ast, body, globals, scope, true, counter);
        }
        Stmt::For { init, body, .. } => {
            for_init_names(init.as_deref(), &mut scope.shadow);
            walk_children(ast, body, globals, scope, true, counter);
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            walk_children(ast, body, globals, scope, true, counter)
        }
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            walk_list(ast, body, globals, scope, true, counter);
            if let Some(cp) = catch_param {
                scope.annexb.push(cp.clone());
                scope.shadow.push(cp.clone());
            }
            walk_list(ast, catch_body, globals, scope, true, counter);
            scope.reset(depth);
            if let Some(fb) = finally_body {
                walk_list(ast, fb, globals, scope, true, counter);
            }
        }
        Stmt::Switch { .. } => super::case_block::walk_case_block(ast, s, globals, scope, counter),
        _ => {}
    }
    scope.reset(depth);
}
