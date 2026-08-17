//! BFS-resolver helpers carved out of [`super::resolve_imports`]:
//! request filtering against per-path injected state, nested-import
//! queueing, `export <decl>` injection, P13-S4 re-export translation,
//! the §16.2.3 star re-export translation and the P13-S2
//! namespace-object materialization.

use crate::ast::{Ast, ExportStar, Expr, Stmt};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use super::{
    NamedImport, WorkItem, check_k2_form, decl_name, is_builtin_module_source, resolve_path,
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
    /// (importer-visible field spelling, injected local binding) —
    /// the two differ when the deconflict pass mangled a colliding
    /// decl (423-01): the FIELD keeps the export name, the value
    /// Ident references the mangled top-level binding.
    order: Vec<(String, String)>,
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

    pub(super) fn fields(&self) -> &[(String, String)] {
        &self.order
    }

    /// Claim the FIELD spelling `name` for this namespace, backed by
    /// the top-level binding `local`. `false` = another module
    /// already contributed the field, so this occurrence is shadowed
    /// and its declaration must NOT inject — a second top-level decl
    /// of the same name is a redeclaration, not an override.
    pub(super) fn claim(&mut self, name: &str, local: &str) -> bool {
        if self.seen.insert(name.to_string()) {
            self.order.push((name.to_string(), local.to_string()));
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
    work.push_back((path, named, default, side_effect_only, namespace, false));
    Ok(())
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
    rename: &HashMap<&str, Vec<&str>>,
    default_alias: &Option<String>,
    ns: Option<&mut NsAccum>,
) -> Result<(), String> {
    if is_builtin_module_source(lib_source) {
        return Ok(());
    }
    let path = resolve_path(target_dir, lib_source)?;
    // A namespace request sees each re-exported FACE as one of this
    // lib's own exports (§16.2.3): the field claims the face spelling
    // and binds to a synthetic `__reex_<ns>_<face>` the nested load
    // materializes — a face spelled by the source module's own name
    // would collide across libs, and named-requested spellings are
    // census-exempt on purpose.
    if let Some(ns) = ns {
        for (orig, alias) in lib_named {
            let face = alias.as_deref().unwrap_or(orig);
            let binding = format!("__reex_{}_{}", ns.alias(), face);
            if !ns.claim(face, &binding) {
                continue; // an earlier walk already owns this field
            }
            if orig == "default" {
                // The source's default export only answers on the
                // default lane — a named request for a binding
                // literally called `default` would miss it.
                work.push_back((path.clone(), Vec::new(), Some(binding), false, None, false));
            } else {
                work.push_back((
                    path.clone(),
                    vec![(orig.clone(), Some(binding))],
                    None,
                    false,
                    None,
                    false,
                ));
            }
        }
        return Ok(());
    }
    // For each (orig, alias) in the lib's re-export
    // clause, the lib exposes `alias` (or `orig` if no
    // alias). We only need to actually load the names
    // the caller asked for via `want` / `default_alias` —
    // one nested request per importer-visible spelling
    // (421-04: `import { x, x as y }` wants BOTH).
    let mut nested_named: Vec<NamedImport> = Vec::new();
    for (orig, alias) in lib_named {
        let lib_visible = alias.as_deref().unwrap_or(orig);
        let final_names: Vec<String> = if lib_visible == "default" {
            let Some(da) = default_alias else {
                continue;
            };
            vec![da.clone()]
        } else if want.contains(lib_visible) {
            rename
                .get(lib_visible)
                .map(|vs| vs.iter().map(|s| (*s).to_string()).collect())
                .unwrap_or_else(|| vec![lib_visible.to_string()])
        } else {
            continue;
        };
        for final_name in final_names {
            if orig == "default" {
                work.push_back((
                    path.clone(),
                    Vec::new(),
                    Some(final_name),
                    false,
                    None,
                    false,
                ));
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
    }
    if !nested_named.is_empty() {
        work.push_back((path, nested_named, None, false, None, false));
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
    rename: &HashMap<&str, Vec<&str>>,
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
            // One namespace load per importer-visible spelling
            // (421-04) — each alias gets its own accumulator.
            for local in star_ns_locals(exported, want, rename, default_alias, ns) {
                work.push_back((path.clone(), Vec::new(), None, false, Some(local), false));
            }
        }
        ExportStar::All => match ns {
            Some(ns) => {
                // `export * from` pour — §16.2.3 excludes `default`; the
                // flag keeps the ns default-field synthesis off this walk.
                work.push_back((
                    path,
                    Vec::new(),
                    None,
                    false,
                    Some(ns.alias().to_string()),
                    true,
                ))
            }
            None => {
                let forwarded: Vec<NamedImport> = want
                    .iter()
                    .filter(|n| !own_exports.contains(**n))
                    .flat_map(|n| match rename.get(*n) {
                        Some(vs) => vs
                            .iter()
                            .map(|a| ((*n).to_string(), (a != n).then(|| (*a).to_string())))
                            .collect::<Vec<_>>(),
                        None => vec![((*n).to_string(), None)],
                    })
                    .collect();
                if !forwarded.is_empty() {
                    work.push_back((path, forwarded, None, false, None, false));
                }
            }
        },
    }
    Ok(())
}

/// The local bindings an `export * as <exported> from "m"` clause has
/// to produce for THIS request — empty when the request never asked
/// for that name, one per importer-visible spelling otherwise
/// (421-04). Under an outer namespace request the field keeps its
/// exported spelling; a named request applies the importer's aliases;
/// `export * as default` answers the importer's default binding.
fn star_ns_locals(
    exported: &str,
    want: &HashSet<&str>,
    rename: &HashMap<&str, Vec<&str>>,
    default_alias: &Option<String>,
    ns: Option<&mut NsAccum>,
) -> Vec<String> {
    if let Some(ns) = ns {
        // Star-forwarded names re-enter the BFS as NAMED requests, so
        // the remote decl keeps its exported spelling (the deconflict
        // census never mangles a requested name) — field == local.
        return ns
            .claim(exported, exported)
            .then(|| exported.to_string())
            .into_iter()
            .collect();
    }
    if exported == "default" {
        return default_alias.clone().into_iter().collect();
    }
    if !want.contains(exported) {
        return Vec::new();
    }
    rename
        .get(exported)
        .map(|vs| vs.iter().map(|a| (*a).to_string()).collect())
        .unwrap_or_else(|| vec![exported.to_string()])
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
    dyn_ns_inline: &mut Vec<(String, Vec<(String, String)>)>,
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
        for (name, local) in accum.fields() {
            let id = ast.add_expr(Expr::Ident(local.clone()));
            fields.push((name.clone(), id));
        }
        let obj_id = ast.add_expr(Expr::ObjectLit { fields });
        // §10.4.6.8 — mark the binding so a member miss on it answers
        // undefined instead of the anonymous-struct typo reject.
        ast.namespace_bindings.insert(alias.clone());
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

/// Dynamic-import namespaces skip the `let` above: the use site may
/// sit inside a (lifted) fn body where a main-local top-level binding
/// is invisible, so each `Ident(__dyn_ns_<n>)` becomes the namespace
/// object literal in place. Field Idents reference the injected lib
/// decls, which named-fn bodies resolve (FnDecls via the pass-1
/// signature hoist, literal consts via the pass-2 pre-pass). The
/// arena scan is name-keyed — sound because `__dyn_ns_<n>` names are
/// parser-minted and arena-offset-seeded, so no other Ident can
/// spell one.
pub(super) fn inline_dyn_ns_objlits(ast: &mut Ast, rewrites: &[(String, Vec<(String, String)>)]) {
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
            for (name, local) in field_names {
                let id = ast.add_expr(Expr::Ident(local.clone()));
                fields.push((name.clone(), id));
            }
            ast.exprs[idx] = Expr::ObjectLit { fields };
        }
    }
}
