//! The nested-class hoist's descent: which child statement containers
//! of one statement it enters, and whether each can run more than once
//! (a function, loop or class body — `ast::class_unit`'s question).
//! Split from `hoist_nested_classes.rs` when the unit-class admission
//! pushed it past the 500-line limit.

use super::hoist_nested_classes::walk_container;
use super::*;

/// Recurse into every child statement container of one statement.
/// `Box<Stmt>` positions (if/while bodies) recurse through here too;
/// a bare ClassDecl in single-statement position is a spec syntax
/// error and is left alone (stays loud).
pub(super) fn walk_child(
    ast: &mut Ast,
    s: &mut Stmt,
    top_names: &mut Vec<String>,
    hoisted: &mut Vec<Stmt>,
    counter: &mut u32,
    name_counts: &std::collections::HashMap<String, u32>,
    multi_eval: bool,
) {
    match s {
        Stmt::FnDecl { body, .. } => {
            walk_container(ast, body, top_names, hoisted, counter, name_counts, true)
        }
        Stmt::Block(v) | Stmt::Multi(v) => {
            walk_container(ast, v, top_names, hoisted, counter, name_counts, multi_eval)
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_child(
                ast,
                then_branch,
                top_names,
                hoisted,
                counter,
                name_counts,
                multi_eval,
            );
            if let Some(eb) = else_branch {
                walk_child(
                    ast,
                    eb,
                    top_names,
                    hoisted,
                    counter,
                    name_counts,
                    multi_eval,
                );
            }
        }
        // A loop body runs once per iteration.
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. } => {
            walk_child(ast, body, top_names, hoisted, counter, name_counts, true)
        }
        // A label repeats only what it labels, which reaches the loop
        // arm itself.
        Stmt::Labeled { body, .. } => walk_child(
            ast,
            body,
            top_names,
            hoisted,
            counter,
            name_counts,
            multi_eval,
        ),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                walk_child(ast, i, top_names, hoisted, counter, name_counts, true);
            }
            walk_child(ast, body, top_names, hoisted, counter, name_counts, true);
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                walk_container(
                    ast,
                    &mut c.body,
                    top_names,
                    hoisted,
                    counter,
                    name_counts,
                    multi_eval,
                );
            }
            if let Some(d) = default {
                walk_container(ast, d, top_names, hoisted, counter, name_counts, multi_eval);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            walk_container(
                ast,
                body,
                top_names,
                hoisted,
                counter,
                name_counts,
                multi_eval,
            );
            walk_container(
                ast,
                catch_body,
                top_names,
                hoisted,
                counter,
                name_counts,
                multi_eval,
            );
            if let Some(fb) = finally_body {
                walk_container(
                    ast,
                    fb,
                    top_names,
                    hoisted,
                    counter,
                    name_counts,
                    multi_eval,
                );
            }
        }
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            if let Some(c) = ctor {
                walk_container(
                    ast,
                    &mut c.body,
                    top_names,
                    hoisted,
                    counter,
                    name_counts,
                    true,
                );
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                walk_container(
                    ast,
                    &mut m.body,
                    top_names,
                    hoisted,
                    counter,
                    name_counts,
                    true,
                );
            }
            for si in static_init {
                if let StaticInit::Block(v) = si {
                    walk_container(ast, v, top_names, hoisted, counter, name_counts, true);
                }
            }
        }
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => walk_child(
            ast,
            inner,
            top_names,
            hoisted,
            counter,
            name_counts,
            multi_eval,
        ),
        _ => {}
    }
}
