//! The export faces a lib section declares under its own roof: every
//! name it exports, and the names its bare `export { a as b }` clauses
//! give to decls declared elsewhere in the lib.

use crate::ast::{ExportStar, Stmt};
use std::collections::{HashMap, HashSet};

use super::decl_name;

/// The names a lib hands on under its own roof — `export <decl>`,
/// `export { a as b }` with or without a `from`, and the single name
/// an `export * as ns from` clause exposes. A `export * from "m"`
/// must not forward any of these: §16.2.3 resolves a name against the
/// module's own export entries before it looks through a star.
pub(super) fn collect_own_export_names(lib_section: &[Stmt]) -> HashSet<String> {
    let mut names: HashSet<String> = HashSet::new();
    for s in lib_section {
        let Stmt::ExportDecl {
            inner, named, star, ..
        } = s
        else {
            continue;
        };
        if let Some(d) = inner
            && let Some(n) = decl_name(d)
        {
            names.insert(n);
        }
        for (orig, alias) in named {
            names.insert(alias.clone().unwrap_or_else(|| orig.clone()));
        }
        if let Some(ExportStar::AsNamespace(n)) = star {
            names.insert(n.clone());
        }
    }
    names
}

/// P13-S4b — bare named exports (`export { a, b as c }`, no `from`)
/// alias top-level decls declared elsewhere in the lib. Collected
/// orig → exported up front so the decl statements (previously
/// dropped as non-exports) inject when the importer wants them.
pub(super) fn collect_bare_exports(lib_section: &[Stmt]) -> HashMap<String, String> {
    let mut bare_exports: HashMap<String, String> = HashMap::new();
    for s in lib_section {
        if let Stmt::ExportDecl {
            inner: None,
            named,
            default_expr: None,
            source: None,
            star: None,
            type_named,
            ..
        } = s
        {
            for (orig, alias) in named.iter().chain(type_named) {
                bare_exports.insert(orig.clone(), alias.clone().unwrap_or_else(|| orig.clone()));
            }
        }
    }
    bare_exports
}
