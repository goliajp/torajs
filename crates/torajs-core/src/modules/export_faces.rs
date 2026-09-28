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
///
/// One local may be exported under several names (`export { M, M as
/// N }`): each is the same binding (§16.2.1.6.3 ResolveExport). The
/// decl lands once, under its PRIMARY face (the first map); the
/// second map holds the other faces keyed by that primary, for the
/// namespace object's fields and the importer's other spellings.
/// Which face is primary depends on the request: `default` for a
/// default import, else the first face the importer names, else the
/// local's own name (no rename), else the first face written.
pub(super) fn collect_bare_exports(
    lib_section: &[Stmt],
    want: &HashSet<&str>,
    default_first: bool,
) -> (HashMap<String, String>, HashMap<String, Vec<String>>) {
    let mut faces: Vec<(String, Vec<String>)> = Vec::new();
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
                let face = alias.clone().unwrap_or_else(|| orig.clone());
                let at = match faces.iter().position(|(o, _)| o == orig) {
                    Some(at) => at,
                    None => {
                        faces.push((orig.clone(), Vec::new()));
                        faces.len() - 1
                    }
                };
                if !faces[at].1.contains(&face) {
                    faces[at].1.push(face);
                }
            }
        }
    }
    // `export let count` beside `export { count as total }`: the
    // declaration's own name is one more face of the same binding, so
    // it joins the faces the clause gave — the decl then lands once,
    // under whichever face the request picks.
    for s in lib_section {
        if let Stmt::ExportDecl { inner: Some(d), .. } = s
            && let Some(n) = decl_name(d)
            && let Some((_, fs)) = faces.iter_mut().find(|(o, _)| *o == n)
            && !fs.contains(&n)
        {
            fs.push(n);
        }
    }
    let mut primary: HashMap<String, String> = HashMap::new();
    let mut others: HashMap<String, Vec<String>> = HashMap::new();
    for (orig, mut fs) in faces {
        let at = fs
            .iter()
            .position(|f| default_first && f == "default")
            .or_else(|| fs.iter().position(|f| want.contains(f.as_str())))
            .or_else(|| fs.iter().position(|f| *f == orig))
            .unwrap_or(0);
        let face = fs.remove(at);
        if !fs.is_empty() {
            others.insert(face.clone(), fs);
        }
        primary.insert(orig, face);
    }
    (primary, others)
}
