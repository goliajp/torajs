//! BFS-resolver helpers carved out of [`super::resolve_imports`]:
//! request filtering against per-path injected state, nested-import
//! queueing, `export <decl>` injection, P13-S4 re-export translation
//! and the P13-S2 namespace-object materialization.

use crate::ast::{Ast, Expr, Stmt};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use super::{
    NamedImport, WorkItem, check_k2_form, decl_name, is_builtin_module_source, rename_decl,
    resolve_path,
};

/// Filter one worklist request against the per-path injected-name
/// state. The same path may legitimately appear multiple times in
/// the queue (e.g., transitive re-export bringing a new name from a
/// lib we already loaded for a different name); the per-name filter
/// de-duplicates injections without forcing first-encounter wins.
/// Reserves the names about to inject so a sibling queue entry for
/// the same path skips them. Returns `None` when nothing is left to
/// inject.
pub(super) fn filter_request(
    entry: &mut HashSet<String>,
    named: Vec<NamedImport>,
    default_alias: Option<String>,
    namespace_alias: Option<String>,
    side_effect_only: bool,
) -> Option<(Vec<NamedImport>, Option<String>, Option<String>, bool)> {
    let mut effective_named: Vec<NamedImport> = Vec::new();
    for (orig, alias) in &named {
        let visible = alias.as_deref().unwrap_or(orig);
        if !entry.contains(visible) {
            effective_named.push((orig.clone(), alias.clone()));
        }
    }
    let want_default = default_alias.is_some() && !entry.contains("__default");
    let want_namespace = namespace_alias.is_some() && !entry.contains("__namespace");
    let want_se = side_effect_only && !entry.contains("__se");
    if effective_named.is_empty() && !want_default && !want_namespace && !want_se {
        return None;
    }
    for (orig, alias) in &effective_named {
        entry.insert(alias.as_deref().unwrap_or(orig).to_string());
    }
    if want_default {
        entry.insert("__default".into());
    }
    if want_namespace {
        entry.insert("__namespace".into());
    }
    if want_se {
        entry.insert("__se".into());
    }
    Some((
        effective_named,
        if want_default { default_alias } else { None },
        if want_namespace {
            namespace_alias
        } else {
            None
        },
        want_se,
    ))
}

/// Queue one `import` request for the BFS resolver. Built-in module
/// sources (`fs`, `node:fs`, ...) skip filesystem resolution —
/// `desugar_builtin_imports` rewrites their imported names into
/// namespace-method calls before downstream passes run.
pub(super) fn queue_nested_import(
    work: &mut VecDeque<WorkItem>,
    dir: &Path,
    source: &str,
    named: Vec<NamedImport>,
    default: Option<String>,
    namespace: Option<String>,
) -> Result<(), String> {
    if is_builtin_module_source(source) {
        return Ok(());
    }
    check_k2_form(source, &default, &namespace, &named)?;
    let path = resolve_path(dir, source)?;
    let side_effect_only = named.is_empty() && default.is_none() && namespace.is_none();
    work.push_back((path, named, default, side_effect_only, namespace));
    Ok(())
}

/// `export <decl>` (inner Some) injection — type decls always inject;
/// namespace imports inject every value decl under its original name
/// (accumulating `namespace_fields` for the synthetic object literal);
/// named imports filter by `want` and apply `rename`.
pub(super) fn inject_export_inner(
    injections: &mut Vec<Stmt>,
    mut inner: Stmt,
    want: &HashSet<&str>,
    rename: &HashMap<&str, &str>,
    namespace_on: bool,
    namespace_fields: &mut Vec<String>,
) {
    // Type decls always inject — TS doesn't require type
    // names in the value-import list, and downstream
    // check.rs needs them to resolve fn return-type
    // annotations on imported value decls.
    if matches!(inner, Stmt::TypeDecl { .. }) {
        injections.push(inner);
        return;
    }
    // P13-S2 namespace import path: inject every value
    // decl with its original name (no `want` filter, no
    // alias rename) so the synthetic namespace object
    // can reference them. The struct literal builds
    // after the walk finishes.
    if namespace_on && let Some(name) = decl_name(&inner) {
        namespace_fields.push(name);
        injections.push(inner);
        return;
    }
    if let Some(name) = decl_name(&inner)
        && want.contains(name.as_str())
    {
        if let Some(alias) = rename.get(name.as_str()) {
            rename_decl(&mut inner, (*alias).to_string());
        }
        injections.push(inner);
    }
}

/// P13-S4 — `export { a, b as c } from "./other"` re-export.
/// Translate into a transitive BFS load of `./other` with the lib's
/// selected names, BUT swap each name's importer-visible alias so
/// the caller (one level up) sees the same names it requested.
pub(super) fn queue_reexport(
    work: &mut VecDeque<WorkItem>,
    target_dir: &Path,
    lib_named: &[NamedImport],
    lib_source: &str,
    want: &HashSet<&str>,
    rename: &HashMap<&str, &str>,
) -> Result<(), String> {
    if is_builtin_module_source(lib_source) {
        return Ok(());
    }
    let path = resolve_path(target_dir, lib_source)?;
    // For each (orig, alias) in the lib's re-export
    // clause, the lib exposes `alias` (or `orig` if no
    // alias). We only need to actually load the names
    // the caller asked for via `want`.
    let mut nested_named: Vec<NamedImport> = Vec::new();
    for (orig, alias) in lib_named {
        let lib_visible = alias.as_deref().unwrap_or(orig);
        if want.contains(lib_visible) {
            // Final caller-visible name: the importer's
            // own alias (`import { x as y }`) if any,
            // otherwise the lib-visible name.
            let final_name = rename
                .get(lib_visible)
                .map(|s| (*s).to_string())
                .unwrap_or_else(|| lib_visible.to_string());
            // Re-export's nested load fetches `orig` from
            // the source file. The transitive rename
            // alias is `Some(final_name)` when the final
            // name differs from the source-side `orig`;
            // the lib walk's rename map will pick it up.
            let nested_alias = if final_name == *orig {
                None
            } else {
                Some(final_name)
            };
            nested_named.push((orig.clone(), nested_alias));
        }
    }
    if !nested_named.is_empty() {
        work.push_back((path, nested_named, None, false, None));
    }
    Ok(())
}

/// P13-S2 — materialize the namespace alias as a synthetic
/// `let <alias> = { name1: name1, name2: name2, ... }`. Each
/// field's value is an `Ident(name)` referencing the just-
/// injected lib decl, so member access (`<alias>.<name>`)
/// resolves through the struct's field type.
pub(super) fn materialize_namespace(
    ast: &mut Ast,
    injections: &mut Vec<Stmt>,
    ns_name: String,
    namespace_fields: &[String],
) {
    let mut fields: Vec<(String, crate::ast::ExprId)> = Vec::with_capacity(namespace_fields.len());
    for name in namespace_fields {
        let id = ast.add_expr(Expr::Ident(name.clone()));
        fields.push((name.clone(), id));
    }
    let obj_id = ast.add_expr(Expr::ObjectLit { fields });
    injections.push(Stmt::LetDecl {
        mutable: false,
        name: ns_name,
        type_ann: None,
        init: obj_id,
        is_var: false,
    });
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
        } = s
        {
            for (orig, alias) in named {
                bare_exports.insert(orig.clone(), alias.clone().unwrap_or_else(|| orig.clone()));
            }
        }
    }
    bare_exports
}

/// P13-S4b — a top-level decl a bare named export lists injects
/// under its EXPORTED name (then the importer's own alias applies
/// inside `inject_export_inner`). Everything else is a lib-level
/// non-export statement — dropped.
#[allow(clippy::too_many_arguments)]
pub(super) fn inject_bare_exported_decl(
    injections: &mut Vec<Stmt>,
    other: Stmt,
    bare_exports: &HashMap<String, String>,
    want: &HashSet<&str>,
    rename: &HashMap<&str, &str>,
    namespace_on: bool,
    namespace_fields: &mut Vec<String>,
) {
    if let Some(dname) = decl_name(&other)
        && let Some(exported) = bare_exports.get(&dname)
    {
        let mut inner = other;
        if *exported != dname {
            rename_decl(&mut inner, exported.clone());
        }
        inject_export_inner(
            injections,
            inner,
            want,
            rename,
            namespace_on,
            namespace_fields,
        );
    }
}

/// Dynamic-import namespaces skip the `let` above: the use site may
/// sit inside a (lifted) fn body where a main-local top-level binding
/// is invisible, so each `Ident(__dyn_ns_<n>)` becomes the namespace
/// object literal in place. Field Idents reference the injected lib
/// decls, which named-fn bodies resolve (FnDecls via the pass-1
/// signature hoist, literal consts via the pass-2 pre-pass). The
/// arena scan is name-keyed — sound because `__dyn_ns_<n>` names are
/// parser-minted and arena-offset-seeded, so no other Ident can
/// spell one.
pub(super) fn inline_dyn_ns_objlits(ast: &mut Ast, rewrites: &[(String, Vec<String>)]) {
    for (ns_name, field_names) in rewrites {
        let hits: Vec<usize> = ast
            .exprs
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                Expr::Ident(n) if n == ns_name => Some(i),
                _ => None,
            })
            .collect();
        for idx in hits {
            let mut fields: Vec<(String, crate::ast::ExprId)> =
                Vec::with_capacity(field_names.len());
            for name in field_names {
                let id = ast.add_expr(Expr::Ident(name.clone()));
                fields.push((name.clone(), id));
            }
            ast.exprs[idx] = Expr::ObjectLit { fields };
        }
    }
}
