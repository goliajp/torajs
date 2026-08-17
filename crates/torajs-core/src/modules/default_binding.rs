//! The default export's one-binding rule (§16.2.1) — a module's
//! `export default` materializes exactly once; whoever asks first
//! wins the `let`, every later request binds by reference, and a
//! namespace request mints a synthetic binding so the ns object can
//! carry its `default` field (§16.2.1.10). Split from `modules.rs`
//! when the field synthesis pushed it past the 500-line limit: the
//! parent answers "how does the BFS walk", this file answers "how
//! does a default export bind".

use crate::ast::{Ast, Stmt};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The repeat-default shortcut — a later default request against a
/// path whose default export already materialized binds the new
/// alias by reference instead of re-walking (see `default_bound_as`).
pub(super) fn shortcut_repeat_default(
    ast: &mut Ast,
    target_path: &Path,
    default_alias: &mut Option<String>,
    default_bound_as: &HashMap<PathBuf, String>,
    injections_by_path: &mut HashMap<PathBuf, Vec<Stmt>>,
) {
    if let Some(second) = default_alias.as_ref()
        && let Some(first) = default_bound_as.get(target_path)
    {
        if second != first {
            let stmt = bind_repeat_default_alias(ast, first, second);
            injections_by_path
                .entry(target_path.to_path_buf())
                .or_default()
                .push(stmt);
        }
        *default_alias = None;
    }
}

/// A later default request against a path whose default export
/// already materialized (see `default_bound_as` in
/// [`super::resolve_imports`]) — the new alias binds to the first
/// one's `let` instead of re-walking, which would re-lower the same
/// ExprId.
fn bind_repeat_default_alias(ast: &mut Ast, first: &str, second: &str) -> Stmt {
    let init = ast.add_expr(crate::ast::Expr::Ident(first.to_string()));
    Stmt::LetDecl {
        mutable: false,
        name: second.to_string(),
        type_ann: None,
        init,
        is_var: false,
    }
}

/// The whole per-walk default step: mint / reuse the ns `default`
/// binding (see [`synth_ns_default_binding`]) and record which alias
/// this path's default bound under, so a later `import d` binds by
/// reference instead of re-evaluating the ExprId.
pub(super) fn settle_walk_default(
    lib_section: &[Stmt],
    namespace_alias: &Option<String>,
    default_alias: &mut Option<String>,
    default_bound_as: &mut HashMap<PathBuf, String>,
    target_path: &Path,
    ns_star_feed: bool,
) -> Option<String> {
    let repeat = synth_ns_default_binding(
        lib_section,
        namespace_alias,
        default_alias,
        default_bound_as,
        target_path,
        ns_star_feed,
    );
    if let Some(a) = default_alias {
        default_bound_as.insert(target_path.to_path_buf(), a.clone());
    }
    repeat
}

/// §16.2.1.10 — the namespace object always carries a `default` field
/// when the lib has a default export. A namespace request with no
/// default binding of its own materializes it under a synthetic
/// `__nsdefault_<alias>` binding, riding the alias form's machinery
/// (the caller records it in `default_bound_as`, so a later
/// `import d` shortcut-binds instead of re-evaluating). A path whose
/// default already materialized reuses that binding: the return value
/// is the existing binding the field claim is still owed for.
fn synth_ns_default_binding(
    lib_section: &[Stmt],
    namespace_alias: &Option<String>,
    default_alias: &mut Option<String>,
    default_bound_as: &HashMap<PathBuf, String>,
    target_path: &Path,
    ns_star_feed: bool,
) -> Option<String> {
    if ns_star_feed {
        // §16.2.3 — a bare `export * from` pour never forwards
        // `default`, so a star-fed walk of a default-carrying module
        // must not mint the field.
        return None;
    }
    let ns_alias = namespace_alias.as_ref()?;
    if default_alias.is_some() {
        return None;
    }
    if let Some(first) = default_bound_as.get(target_path) {
        return Some(first.clone());
    }
    let has_default = lib_section.iter().any(|s| {
        matches!(
            s,
            Stmt::ExportDecl {
                default_expr: Some(_),
                ..
            }
        )
    });
    if has_default {
        *default_alias = Some(format!("__nsdefault_{ns_alias}"));
    }
    None
}
