//! What a scope binds — the census side of the shadow-aware class
//! reference rewrite (`class_globals_shadow`).
//!
//! Two questions live next door to each other and are easy to
//! confuse: "which names does this walk still see as classes" is the
//! walk's, next door; "which names does this scope rebind" is this
//! one's. Keeping the second here leaves the walk reading as a walk.

use super::Stmt;
use std::collections::HashSet;

pub(super) fn collect_scope_decls(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::LetDecl { name, .. }
            | Stmt::FnDecl { name, .. }
            | Stmt::ClassDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_scope_decls(std::slice::from_ref(then_branch), out);
                if let Some(eb) = else_branch.as_deref() {
                    collect_scope_decls(std::slice::from_ref(eb), out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                collect_scope_decls(std::slice::from_ref(body), out)
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init.as_deref() {
                    collect_scope_decls(std::slice::from_ref(i), out);
                }
                collect_scope_decls(std::slice::from_ref(body), out);
            }
            Stmt::ForOf { var_name, body, .. } => {
                out.insert(var_name.clone());
                collect_scope_decls(std::slice::from_ref(body), out);
            }
            Stmt::ForOfSplitIter { var_name, body, .. } => {
                out.insert(var_name.clone());
                collect_scope_decls(std::slice::from_ref(body), out);
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_scope_decls(body, out);
                collect_scope_decls(catch_body, out);
                if let Some(fb) = finally_body {
                    collect_scope_decls(fb, out);
                }
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    collect_scope_decls(&c.body, out);
                }
                if let Some(d) = default {
                    collect_scope_decls(d, out);
                }
            }
            Stmt::Block(b) | Stmt::Multi(b) => collect_scope_decls(b, out),
            _ => {}
        }
    }
}
