//! The descent half of the nested-fn lift — how the walk reaches a
//! declaration, kept apart from what it does when it finds one.
//!
//! Every arm here is the same statement: recurse into whatever block-
//! shaped children this statement has. The sibling
//! [`super::nested_fns`] owns the other half — the lift itself, the
//! mangling, and the Annex B §B.3.3 var write.

use super::Stmt;
use super::annexb_fn_var::{LiftCtx, LiftMode};
use super::nested_fns::{collect_nested_fns_in_stmt, collect_nested_fns_to_lift};
use std::collections::{HashMap, HashSet};

/// Recurse into `stmt`'s block-shaped children. Leaf statements have
/// no nested `FnDecl` to find.
pub(super) fn descend_into_children(
    stmt: &mut Stmt,
    parent_name: &str,
    renames: &mut HashMap<String, String>,
    branch_scoped: &mut HashSet<String>,
    mode: LiftMode,
    lifted: &mut Vec<Stmt>,
    ctx: &mut LiftCtx,
) {
    // Every arm but `Multi` opens a scope, so a `function` found under
    // it is nested. `Multi` is a compiler-generated sequence sharing the
    // SURROUNDING scope (a sloppy eval's hoisted declarations arrive in
    // one), so what it holds is at the caller's own depth.
    match stmt {
        Stmt::Multi(body) => {
            collect_nested_fns_to_lift(
                body,
                parent_name,
                renames,
                branch_scoped,
                mode,
                lifted,
                ctx,
            );
        }
        Stmt::Block(body) => {
            collect_nested_fns_to_lift(
                body,
                parent_name,
                renames,
                branch_scoped,
                mode.descend(),
                lifted,
                ctx,
            );
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_nested_fns_in_stmt(
                then_branch,
                parent_name,
                renames,
                branch_scoped,
                mode.descend(),
                lifted,
                ctx,
            );
            if let Some(eb) = else_branch {
                collect_nested_fns_in_stmt(
                    eb,
                    parent_name,
                    renames,
                    branch_scoped,
                    mode.descend(),
                    lifted,
                    ctx,
                );
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_nested_fns_in_stmt(
                body,
                parent_name,
                renames,
                branch_scoped,
                mode.descend(),
                lifted,
                ctx,
            );
        }
        Stmt::Labeled { body, .. } => {
            collect_nested_fns_in_stmt(
                body,
                parent_name,
                renames,
                branch_scoped,
                mode.descend(),
                lifted,
                ctx,
            );
        }
        // A loop head is a scope of its own: `for (let f; ;)` and
        // `for (let f of …)` both put `f` in scope for the body, which
        // is the early error §B.3.3.1 asks about.
        Stmt::For { init, body, .. } => {
            let mark = match init.as_deref() {
                Some(Stmt::LetDecl {
                    name,
                    is_var: false,
                    ..
                }) => ctx.enter_binding(name),
                _ => ctx.enter_binding_none(),
            };
            collect_nested_fns_in_stmt(
                body,
                parent_name,
                renames,
                branch_scoped,
                mode.descend(),
                lifted,
                ctx,
            );
            ctx.leave_scope(mark);
        }
        Stmt::ForOfSplitIter { var_name, body, .. } | Stmt::ForOf { var_name, body, .. } => {
            let mark = ctx.enter_binding(var_name);
            collect_nested_fns_in_stmt(
                body,
                parent_name,
                renames,
                branch_scoped,
                mode.descend(),
                lifted,
                ctx,
            );
            ctx.leave_scope(mark);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_nested_fns_to_lift(
                body,
                parent_name,
                renames,
                branch_scoped,
                mode.descend(),
                lifted,
                ctx,
            );
            collect_nested_fns_to_lift(
                catch_body,
                parent_name,
                renames,
                branch_scoped,
                mode.descend(),
                lifted,
                ctx,
            );
            if let Some(fb) = finally_body {
                collect_nested_fns_to_lift(
                    fb,
                    parent_name,
                    renames,
                    branch_scoped,
                    mode.descend(),
                    lifted,
                    ctx,
                );
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for case in cases.iter_mut() {
                collect_nested_fns_to_lift(
                    &mut case.body,
                    parent_name,
                    renames,
                    branch_scoped,
                    mode.descend(),
                    lifted,
                    ctx,
                );
            }
            if let Some(d) = default {
                collect_nested_fns_to_lift(
                    d,
                    parent_name,
                    renames,
                    branch_scoped,
                    mode.descend(),
                    lifted,
                    ctx,
                );
            }
        }
        _ => {} // leaf stmts have no nested FnDecl children
    }
}
