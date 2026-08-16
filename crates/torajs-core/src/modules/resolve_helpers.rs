//! BFS-resolver helpers carved out of [`super::resolve_imports`]:
//! request filtering against per-path injected state, nested-import
//! queueing, `export <decl>` injection, P13-S4 re-export translation,
//! the §16.2.3 star re-export translation and the P13-S2
//! namespace-object materialization.

use crate::ast::{Ast, ExportStar, Expr, Stmt};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use super::{
    NamedImport, WorkItem, check_k2_form, decl_name, is_builtin_module_source, rename_decl,
    resolve_path,
};

/// Field accumulator for one namespace alias (`import * as ns from …`).
///
/// Fields accumulate per ALIAS, not per module: a lib's
/// `export * from "m"` re-exports m's names into the SAME namespace
/// object, and m is a separate BFS work item popped later. Claiming is
/// first-writer-wins, and BFS order puts the module closest to the
/// importer first — which is §16.2.3's rule that a module's own export
/// shadows one reached through a star.
pub(super) struct NsAccum {
    alias: String,
    order: Vec<String>,
    seen: HashSet<String>,
}

impl NsAccum {
    pub(super) fn new(alias: String) -> Self {
        Self {
            alias,
            order: Vec::new(),
            seen: HashSet::new(),
        }
    }

    /// The importer-visible binding this object will be assigned to.
    pub(super) fn alias(&self) -> &str {
        &self.alias
    }

    pub(super) fn fields(&self) -> &[String] {
        &self.order
    }

    /// Claim `name` for this namespace. `false` = another module
    /// already contributed it, so this occurrence is shadowed and its
    /// declaration must NOT inject — a second top-level decl of the
    /// same name is a redeclaration, not an override.
    pub(super) fn claim(&mut self, name: &str) -> bool {
        if self.seen.insert(name.to_string()) {
            self.order.push(name.to_string());
            true
        } else {
            false
        }
    }
}

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
    ns: Option<&mut NsAccum>,
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
    // after the BFS drains.
    if let Some(ns) = ns
        && let Some(name) = decl_name(&inner)
    {
        if ns.claim(&name) {
            injections.push(inner);
        }
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
///
/// `default` can sit on either side of a specifier, and each side
/// means a different lane. As the EXPOSED name (`export { V as
/// default }`, `export { default }`) the clause answers the
/// importer's default binding, so the request that selects it is
/// `default_alias`, not `want`. As the SOURCE name (`export
/// { default as X }`, `export { default }`) the nested load has to
/// fetch the other module's default export — a named request for a
/// binding literally called `default` would miss it — so it queues on
/// the default lane under the final caller-visible name.
pub(super) fn queue_reexport(
    work: &mut VecDeque<WorkItem>,
    target_dir: &Path,
    lib_named: &[NamedImport],
    lib_source: &str,
    want: &HashSet<&str>,
    rename: &HashMap<&str, &str>,
    default_alias: &Option<String>,
) -> Result<(), String> {
    if is_builtin_module_source(lib_source) {
        return Ok(());
    }
    let path = resolve_path(target_dir, lib_source)?;
    // For each (orig, alias) in the lib's re-export
    // clause, the lib exposes `alias` (or `orig` if no
    // alias). We only need to actually load the names
    // the caller asked for via `want` / `default_alias`.
    let mut nested_named: Vec<NamedImport> = Vec::new();
    for (orig, alias) in lib_named {
        let lib_visible = alias.as_deref().unwrap_or(orig);
        // Final caller-visible name: the importer's own
        // alias (`import { x as y }`) if any, otherwise
        // the lib-visible name.
        let final_name = if lib_visible == "default" {
            let Some(da) = default_alias else {
                continue;
            };
            da.clone()
        } else if want.contains(lib_visible) {
            rename
                .get(lib_visible)
                .map(|s| (*s).to_string())
                .unwrap_or_else(|| lib_visible.to_string())
        } else {
            continue;
        };
        if orig == "default" {
            work.push_back((path.clone(), Vec::new(), Some(final_name), false, None));
            continue;
        }
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
    if !nested_named.is_empty() {
        work.push_back((path, nested_named, None, false, None));
    }
    Ok(())
}

/// §16.2.3 — a `export * from "m"` / `export * as ns from "m"` clause
/// met while walking a lib.
///
/// `All` forwards the importer's own request on to `m`: a star
/// preserves names, so the wanted set translates unchanged — minus
/// whatever this lib exports itself, because a local export shadows
/// the star's. Under a namespace request there is no wanted set, so
/// `m` walks as a namespace load under the SAME alias and its exports
/// land in the same accumulator.
///
/// `AsNamespace` exposes exactly one name (the namespace object), so
/// it always queues `m` as a namespace load — under the importer's
/// alias when the importer asked for that name, under the exported
/// spelling when it is a field of an outer namespace. `default` is a
/// legal ModuleExportName here, and then the name it answers is the
/// importer's default binding.
#[allow(clippy::too_many_arguments)]
pub(super) fn queue_star_reexport(
    work: &mut VecDeque<WorkItem>,
    target_dir: &Path,
    star: &ExportStar,
    lib_source: &str,
    want: &HashSet<&str>,
    rename: &HashMap<&str, &str>,
    default_alias: &Option<String>,
    own_exports: &HashSet<String>,
    ns: Option<&mut NsAccum>,
) -> Result<(), String> {
    if is_builtin_module_source(lib_source) {
        return Ok(());
    }
    let path = resolve_path(target_dir, lib_source)?;
    match star {
        ExportStar::AsNamespace(exported) => {
            let Some(local) = star_ns_local(exported, want, rename, default_alias, ns) else {
                return Ok(());
            };
            work.push_back((path, Vec::new(), None, false, Some(local)));
        }
        ExportStar::All => match ns {
            Some(ns) => {
                work.push_back((path, Vec::new(), None, false, Some(ns.alias().to_string())))
            }
            None => {
                let forwarded: Vec<NamedImport> = want
                    .iter()
                    .filter(|n| !own_exports.contains(**n))
                    .map(|n| ((*n).to_string(), rename.get(*n).map(|a| (*a).to_string())))
                    .collect();
                if !forwarded.is_empty() {
                    work.push_back((path, forwarded, None, false, None));
                }
            }
        },
    }
    Ok(())
}

/// The local binding an `export * as <exported> from "m"` clause has
/// to produce for THIS request, or `None` when the request never asked
/// for that name. Under an outer namespace request the field keeps its
/// exported spelling; a named request applies the importer's alias;
/// `export * as default` answers the importer's default binding.
fn star_ns_local(
    exported: &str,
    want: &HashSet<&str>,
    rename: &HashMap<&str, &str>,
    default_alias: &Option<String>,
    ns: Option<&mut NsAccum>,
) -> Option<String> {
    if let Some(ns) = ns {
        return ns.claim(exported).then(|| exported.to_string());
    }
    if exported == "default" {
        return default_alias.clone();
    }
    if !want.contains(exported) {
        return None;
    }
    Some(
        rename
            .get(exported)
            .map(|a| (*a).to_string())
            .unwrap_or_else(|| exported.to_string()),
    )
}

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

/// P13-S2 — materialize each namespace alias as a synthetic
/// `let <alias> = { name1: name1, name2: name2, ... }`. Each field's
/// value is an `Ident(name)` referencing an already-injected lib decl,
/// so member access (`<alias>.<name>`) resolves through the struct's
/// field type.
///
/// Emission waits until the BFS has drained — that is what lets a
/// star re-export contribute to a namespace whose own lib was walked
/// several work items earlier. Order is REVERSE discovery: a namespace
/// can only name another that was discovered while walking it
/// (`export * as inner from "m"`), so the later-discovered one has to
/// bind first.
pub(super) fn materialize_pending_namespaces(
    ast: &mut Ast,
    injections: &mut Vec<Stmt>,
    order: &[String],
    accums: &HashMap<String, NsAccum>,
    dyn_ns_inline: &mut Vec<(String, Vec<String>)>,
) {
    for alias in order.iter().rev() {
        let Some(accum) = accums.get(alias) else {
            continue;
        };
        if alias.starts_with("__dyn_ns_") {
            dyn_ns_inline.push((alias.clone(), accum.fields().to_vec()));
            continue;
        }
        let mut fields: Vec<(String, crate::ast::ExprId)> =
            Vec::with_capacity(accum.fields().len());
        for name in accum.fields() {
            let id = ast.add_expr(Expr::Ident(name.clone()));
            fields.push((name.clone(), id));
        }
        let obj_id = ast.add_expr(Expr::ObjectLit { fields });
        injections.push(Stmt::LetDecl {
            mutable: false,
            name: alias.clone(),
            type_ann: None,
            init: obj_id,
            is_var: false,
        });
    }
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
pub(super) fn inject_bare_exported_decl(
    injections: &mut Vec<Stmt>,
    other: Stmt,
    bare_exports: &HashMap<String, String>,
    want: &HashSet<&str>,
    rename: &HashMap<&str, &str>,
    ns: Option<&mut NsAccum>,
) {
    if let Some(dname) = decl_name(&other)
        && let Some(exported) = bare_exports.get(&dname)
    {
        let mut inner = other;
        if *exported != dname {
            rename_decl(&mut inner, exported.clone());
        }
        inject_export_inner(injections, inner, want, rename, ns);
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
