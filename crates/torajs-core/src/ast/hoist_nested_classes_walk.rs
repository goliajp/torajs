//! The nested-class hoist's descent: which child statement containers
//! of one statement it enters, and whether each can run more than once
//! (a function, loop or class body — `ast::class_unit`'s question).
//! Split from `hoist_nested_classes.rs` when the unit-class admission
//! pushed it past the 500-line limit.

use super::hoist_nested_classes::{HoistNames, walk_container};
use super::scope_chain::{fn_names, for_init_names};
use super::*;

/// Recurse into every child statement container of one statement.
/// `Box<Stmt>` positions (if/while bodies) recurse through here too;
/// a bare ClassDecl in single-statement position is a spec syntax
/// error and is left alone (stays loud).
///
/// Each frame a child opens adds what it binds to `names.shadow` for
/// the walk under it, so the capture check knows which top-level names
/// it rebinds (`scope_chain`).
pub(super) fn walk_child(
    ast: &mut Ast,
    s: &mut Stmt,
    names: &mut HoistNames,
    hoisted: &mut Vec<Stmt>,
    counter: &mut u32,
    name_counts: &super::hoist_nested_classes_census::NameCensus,
    multi_eval: bool,
) {
    let depth = names.shadow.len();
    match s {
        Stmt::FnDecl { params, body, .. } => {
            fn_names(params, body, &mut names.shadow);
            walk_container(ast, body, names, hoisted, counter, name_counts, true)
        }
        Stmt::Block(v) | Stmt::Multi(v) => {
            walk_container(ast, v, names, hoisted, counter, name_counts, multi_eval)
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_child(
                ast,
                then_branch,
                names,
                hoisted,
                counter,
                name_counts,
                multi_eval,
            );
            if let Some(eb) = else_branch {
                walk_child(ast, eb, names, hoisted, counter, name_counts, multi_eval);
            }
        }
        // A loop body runs once per iteration.
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            walk_child(ast, body, names, hoisted, counter, name_counts, true)
        }
        Stmt::ForOfSplitIter { var_name, body, .. } | Stmt::ForOf { var_name, body, .. } => {
            names.shadow.push(var_name.clone());
            walk_child(ast, body, names, hoisted, counter, name_counts, true)
        }
        // A label repeats only what it labels, which reaches the loop
        // arm itself.
        Stmt::Labeled { body, .. } => {
            walk_child(ast, body, names, hoisted, counter, name_counts, multi_eval)
        }
        Stmt::For { init, body, .. } => {
            for_init_names(init.as_deref(), &mut names.shadow);
            if let Some(i) = init {
                walk_child(ast, i, names, hoisted, counter, name_counts, true);
            }
            walk_child(ast, body, names, hoisted, counter, name_counts, true);
        }
        Stmt::Switch { cases, default, .. } => {
            // One block holds every clause; a clause's classes are its
            // own container's siblings.
            for list in cases.iter().map(|c| &c.body[..]).chain(default.as_deref()) {
                super::scope_chain::list_names_but_classes(list, &mut names.shadow);
            }
            for c in cases {
                walk_container(
                    ast,
                    &mut c.body,
                    names,
                    hoisted,
                    counter,
                    name_counts,
                    multi_eval,
                );
            }
            if let Some(d) = default {
                walk_container(ast, d, names, hoisted, counter, name_counts, multi_eval);
            }
        }
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            walk_container(ast, body, names, hoisted, counter, name_counts, multi_eval);
            names.shadow.extend(catch_param.iter().cloned());
            walk_container(
                ast,
                catch_body,
                names,
                hoisted,
                counter,
                name_counts,
                multi_eval,
            );
            names.shadow.truncate(depth);
            if let Some(fb) = finally_body {
                walk_container(ast, fb, names, hoisted, counter, name_counts, multi_eval);
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
                fn_names(&c.params, &c.body, &mut names.shadow);
                walk_container(ast, &mut c.body, names, hoisted, counter, name_counts, true);
                names.shadow.truncate(depth);
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                fn_names(&m.params, &m.body, &mut names.shadow);
                walk_container(ast, &mut m.body, names, hoisted, counter, name_counts, true);
                names.shadow.truncate(depth);
            }
            for si in static_init {
                if let StaticInit::Block(v) = si {
                    fn_names(&[], v, &mut names.shadow);
                    walk_container(ast, v, names, hoisted, counter, name_counts, true);
                    names.shadow.truncate(depth);
                }
            }
        }
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => walk_child(ast, inner, names, hoisted, counter, name_counts, multi_eval),
        _ => {}
    }
    names.shadow.truncate(depth);
}
