//! Program-wide `ClassDecl` name census (406-02) — a computed STATIC
//! field's side-table rows match by class NAME and leave no trace on
//! the class itself, so the capturing lane may install them only when
//! the name provably has one owner. Split from
//! `hoist_nested_classes.rs` at the file-size cap; the walk mirrors
//! `walk_child`'s container coverage, class bodies included (a
//! same-named class may hide inside a method body).

use super::*;

pub(super) fn count_class_decl_names(s: &Stmt, out: &mut std::collections::HashMap<String, u32>) {
    if let Stmt::ClassDecl {
        name,
        ctor,
        methods,
        static_methods,
        static_init,
        ..
    } = s
    {
        *out.entry(name.clone()).or_insert(0) += 1;
        if let Some(c) = ctor {
            for cs in &c.body {
                count_class_decl_names(cs, out);
            }
        }
        for m in methods.iter().chain(static_methods.iter()) {
            for ms in &m.body {
                count_class_decl_names(ms, out);
            }
        }
        for si in static_init {
            if let StaticInit::Block(v) = si {
                for vs in v {
                    count_class_decl_names(vs, out);
                }
            }
        }
        return;
    }
    match s {
        Stmt::FnDecl { body, .. } | Stmt::Block(body) | Stmt::Multi(body) => {
            for cs in body {
                count_class_decl_names(cs, out);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            count_class_decl_names(then_branch, out);
            if let Some(eb) = else_branch {
                count_class_decl_names(eb, out);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::Labeled { body, .. } => count_class_decl_names(body, out),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                count_class_decl_names(i, out);
            }
            count_class_decl_names(body, out);
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for cs in &c.body {
                    count_class_decl_names(cs, out);
                }
            }
            if let Some(d) = default {
                for cs in d {
                    count_class_decl_names(cs, out);
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for cs in body.iter().chain(catch_body.iter()) {
                count_class_decl_names(cs, out);
            }
            if let Some(fb) = finally_body {
                for cs in fb {
                    count_class_decl_names(cs, out);
                }
            }
        }
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => count_class_decl_names(inner, out),
        _ => {}
    }
}
