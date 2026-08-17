//! 423-01 — the name face of one top-level declaration (through the
//! `export` wrapper): read it, classify it, point it somewhere else.
//! Split from `deconflict.rs` when knife D's 427-02 arm hit the
//! 500-line limit; the parent decides WHETHER a decl renames, this
//! file knows WHERE a decl's name lives.

use crate::ast::Stmt;

/// The FnDecl / LetDecl / ClassDecl name a top-level lib statement
/// declares (through the `export` wrapper). Type decls answer None.
/// Knife D admits ClassDecl: the census renames a class through
/// `class_rename::rename_class_artifacts`, which moves every baked
/// artifact with the decl name.
pub(in crate::modules) fn top_value_decl_name(s: &Stmt) -> Option<String> {
    match s {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => top_value_decl_name(inner),
        Stmt::FnDecl { name, .. } | Stmt::LetDecl { name, .. } | Stmt::ClassDecl { name, .. } => {
            Some(name.clone())
        }
        _ => None,
    }
}

pub(in crate::modules) fn is_class_decl(s: &Stmt) -> bool {
    match s {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => is_class_decl(inner),
        Stmt::ClassDecl { .. } => true,
        _ => false,
    }
}

/// Point the declaration at its mangled name (through the `export`
/// wrapper). For a ClassDecl this is only the NAME FIELD — the
/// caller must follow with `rename_class_artifacts`, which owns the
/// baked-artifact move (never rename a class through this alone).
pub(in crate::modules) fn rename_top_decl(s: &mut Stmt, mangled: &str) {
    match s {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => rename_top_decl(inner, mangled),
        Stmt::FnDecl { name, .. } | Stmt::LetDecl { name, .. } | Stmt::ClassDecl { name, .. } => {
            *name = mangled.to_string()
        }
        _ => {}
    }
}
