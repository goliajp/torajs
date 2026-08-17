//! Phase K.2 — cross-file imports.
//!
//! Walks the main file's `ImportDecl`s, reads the targeted files, parses
//! them into the same `Ast` arena via `parser::parse_into` (so ExprIds
//! stay coherent), and prepends each imported file's requested
//! `export`-wrapped declarations to the main file's stmts. Imports are
//! resolved breadth-first, spliced back in dependency post-order (ES
//! §16.2.1.5 — a module body runs after every module it requests; see
//! `modules/graph.rs`); the visited set keys on the canonicalized
//! absolute path so cyclic imports terminate naturally.
//!
//! Subset boundary (P13 sub-step ship chain progressively lifts these):
//!   - `import { a, b as c } from "./y"`     — supported (K.2 baseline)
//!   - `import x from "./y"`                  — supported (P13-S1; pairs with
//!                                              `export default <expr>`)
//!   - `import * as ns from "./y"`            — supported (P13-S2; lib's
//!                                              every value export injects
//!                                              under its original name and a
//!                                              synthetic `let ns = { … }`
//!                                              object literal lands after)
//!   - `import "./y"`                         — supported (P13-S3; lib's
//!                                              top-level stmts inject in
//!                                              source order, no symbols
//!                                              bound on the importer side)
//!   - `export { a, b } from "./y"`           — supported (P13-S4; lib b's
//!                                              re-export pushes a nested BFS
//!                                              load of y with the request
//!                                              translated through the lib's
//!                                              optional alias clause)
//!   - `export * from "./y"`                  — supported; a star preserves
//!                                              names, so the importer's own
//!                                              request forwards to y minus
//!                                              whatever b exports itself, and
//!                                              a namespace request walks y
//!                                              into the SAME namespace
//!                                              accumulator
//!   - `export * as ns from "./y"`            — supported; exposes exactly one
//!                                              name, whose value is y's
//!                                              namespace object
//!   - `export { a, b }` (inline, no source)  — rejected (no module
//!                                              relationship; a plain re-bind
//!                                              of in-scope names; out of K.2
//!                                              scope)
//!
//! Behaviour notes:
//!   - For named-import / default-import forms, lib-level non-`export`
//!     top-level statements are silently dropped — a lib is treated as
//!     a pure declaration source. This is a documented subset deviation
//!     from bun's runtime-evaluation semantics; bench and conformance
//!     avoid relying on it.
//!   - For side-effect imports (`import "./y"`), the lib's full top-
//!     level statement list IS injected in source order (P13-S3). This
//!     matches bun's "module body executes once" semantics for the
//!     side-effect-only invocation. When the same lib is visited via
//!     both forms, the first BFS encounter wins.
//!   - Lib's `export type T = ...` declarations are always injected,
//!     irrespective of whether the importer listed `T` in its named
//!     list — TS itself doesn't require type names to appear in the
//!     value-import list, and check.rs needs the `TypeDecl` to resolve
//!     return-type annotations on imported functions.
//!   - `as <alias>` renames the injected decl AND rewrites a fn's
//!     free self-references to the new spelling (the old K.2 corner —
//!     a renamed recursive fn read its original name, no longer in
//!     scope — closed when 421-04 multi-alias fan-out landed). One
//!     decl injects per importer-visible spelling: `import { fa,
//!     fa as renamed }` binds both, the plain spelling keeping the
//!     original decl, fn aliases deep-cloning, const aliases binding
//!     by reference.

use crate::ast::{Ast, Stmt};
use crate::lexer;
use crate::parser;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

/// One named entry in an `import` clause: `(orig_name, alias)`. `alias`
/// is `Some` for `import { foo as bar }`, `None` otherwise.
pub(super) type NamedImport = (String, Option<String>);

/// Worklist entry for the BFS resolver — the canonicalized absolute
/// path of a module to load, plus the named-imports list that drove
/// the request, plus the default-binding alias (`Some(x)` when the
/// importer wrote `import x from "./y"`; the lib's
/// `export default <expr>` resolves to a synthetic `let x = <expr>`),
/// plus a `side_effect_only` flag (`true` when the importer wrote
/// bare `import "./y"`; the lib's full top-level statement list is
/// injected in source order regardless of `export` wrapping), plus
/// the namespace alias (`Some(M)` when the importer wrote
/// `import * as M from "./y"`; the lib's full export list materializes
/// as a synthetic `let M = { name1: name1, name2: name2, ... }`
/// after the named decls inject).
/// (path, named, default_alias, side_effect_only, namespace_alias,
/// ns_star_feed). The last flag marks a `export * from "m"` pour into
/// an existing namespace accumulator: §16.2.3 star forwarding
/// excludes `default`, so the ns `default`-field synthesis must skip
/// these walks (a DIRECT namespace request and `export * as ns` both
/// carry the module's own default and stay false).
type WorkItem = (
    PathBuf,
    Vec<NamedImport>,
    Option<String>,
    bool,
    Option<String>,
    bool,
);

/// The modules the ENTRY file requests, in clause order, onto the
/// worklist. Its own `import`s plus the target of any `export *`: a
/// star re-export in the entry binds nothing anyone can see — nothing
/// imports the entry — but §16.2.1.5 still evaluates the module it
/// names, so it loads for its side effects.
fn seed_entry_requests(
    ast: &Ast,
    base_dir: &Path,
    work: &mut VecDeque<WorkItem>,
) -> Result<(), String> {
    for s in &ast.stmts {
        match s {
            Stmt::ImportDecl {
                source,
                named,
                default,
                namespace,
            } => {
                queue_nested_import(
                    work,
                    base_dir,
                    source,
                    named.clone(),
                    default.clone(),
                    namespace.clone(),
                )?;
            }
            Stmt::ExportDecl {
                star: Some(_),
                source: Some(star_source),
                ..
            } => {
                queue_nested_import(work, base_dir, star_source, Vec::new(), None, None)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Resolve every `import` in `ast` by reading + parsing the target file
/// and injecting its requested named exports as top-level declarations
/// at the front of `ast.stmts`. Single-file mode (no `ImportDecl`s) is
/// a no-op.
///
/// Returns the list of `(canonical_path, source_bytes)` for every file
/// visited (BFS order, deduplicated). `tr run`'s cache key includes
/// every entry so an edit to a transitively-imported file invalidates
/// the cache slot for the main file.
pub fn resolve_imports(ast: &mut Ast, base_dir: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let mut closure_files: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    // Per-path "already injected" tracking. The set holds importer-
    // visible names (= alias if Some, else orig) plus the sentinels
    // `__default` / `__namespace` / `__se` for the non-named import
    // forms. A path may be re-visited via a different request shape
    // (e.g., a transitive re-export's nested load asking for a name
    // not in the first visit's named list); the per-name filter
    // de-duplicates injections without forcing first-encounter wins.
    let mut injected_names: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    let mut closure_paths: HashSet<PathBuf> = HashSet::new();
    let mut work: VecDeque<WorkItem> = VecDeque::new();
    // §16.2.1.5 — a module body runs AFTER every module it requests.
    // The worklist is breadth-first, so pop order is the opposite; the
    // graph is recorded here instead and the injected statements are
    // spliced back in dependency post-order once the queue drains. An
    // edge is whatever a walk pushed onto the worklist, so it needs no
    // threading: [`ModuleGraph::edges_since`] reads the tail.
    let mut graph = ModuleGraph::default();

    // 423-01 deconflict state — the committed-name set (entry decls +
    // entry import bindings, see `deconflict::seed_seen_names`) plus
    // each path's mangle memory (the same file re-parses per request,
    // and the same decl must keep the same mangle).
    let mut seen_names = deconflict::seed_seen_names(ast, base_dir);
    let mut mangled_by_path: HashMap<PathBuf, HashMap<String, String>> = HashMap::new();
    let mut mangle_seq: usize = 0;

    seed_entry_requests(ast, base_dir, &mut work)?;
    // §13.3.10 dynamic import — candidates queue before `graph.roots`
    // records so their injections ride the dependency-order splice.
    let dyn_table = dyn_import::seed_candidates(ast, base_dir, &mut work);
    graph.roots = graph.edges_since(&work, 0);

    // Statements injected on behalf of one module, keyed by its path
    // so the dependency-order splice below can reassemble them. A path
    // may be walked more than once (a later request asking for a name
    // the first visit did not want); the later visit appends.
    let mut injections_by_path: HashMap<PathBuf, Vec<Stmt>> = HashMap::new();
    // Dynamic-import namespaces (`__dyn_ns_<n>`) don't materialize a
    // top-level `let` — a lifted async-fn body can't see main-local
    // bindings, so the resolver rewrites each `Ident(__dyn_ns_<n>)`
    // use site into the namespace object literal directly (the field
    // Idents resolve against the injected lib decls from any fn
    // body). Field lists are remembered per path so a second
    // `import("...")` of an already-visited module (its request is
    // sentinel-deduped to nothing) still gets its rewrite.
    let mut dyn_ns_inline: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut ns_fields_by_path: HashMap<PathBuf, Vec<(String, String)>> = HashMap::new();
    // Namespace object fields, keyed by the ALIAS rather than the
    // module — a lib's `export * from "m"` pours m's exports into the
    // same namespace, and m is a work item popped later. `ns_order`
    // keeps discovery order for [`materialize_pending_namespaces`].
    let mut ns_accums: HashMap<String, NsAccum> = HashMap::new();
    let mut ns_order: Vec<String> = Vec::new();
    // The alias a path's default export materialized under. A module's
    // default is ONE export but nothing stops several requests wanting
    // it under different names (`export { default as X } from "m"`
    // beside `export { default } from "m"`); the first request wins
    // the `let <alias> = <expr>` and every later alias binds to the
    // first — re-walking would re-lower the same ExprId.
    let mut default_bound_as: HashMap<PathBuf, String> = HashMap::new();

    while let Some((
        target_path,
        named,
        mut default_alias,
        side_effect_only,
        mut namespace_alias,
        ns_star_feed,
    )) = work.pop_front()
    {
        if let Some(ns) = namespace_alias
            .as_ref()
            .filter(|n| n.starts_with("__dyn_ns_"))
            && let Some(fields) = ns_fields_by_path.get(&target_path)
        {
            dyn_ns_inline.push((ns.clone(), fields.clone()));
            namespace_alias = None;
        }
        default_binding::shortcut_repeat_default(
            ast,
            &target_path,
            &mut default_alias,
            &default_bound_as,
            &mut injections_by_path,
        );
        // Filter the request against per-path injected state — see
        // [`filter_request`].
        let entry = injected_names.entry(target_path.clone()).or_default();
        let Some((named, mut default_alias, namespace_alias, side_effect_only)) = filter_request(
            entry,
            named,
            default_alias,
            namespace_alias,
            side_effect_only,
        ) else {
            continue;
        };

        let src_text = std::fs::read_to_string(&target_path)
            .map_err(|e| format!("import {}: {e}", target_path.display()))?;
        let tokens = lexer::tokenize(&src_text)
            .map_err(|e| format!("import {} lex: {e}", target_path.display()))?;
        let lib_expr_offset = ast.exprs.len();
        let lib_offset = parser::parse_into(&src_text, &tokens, ast)
            .map_err(|e| format!("import {} parse: {e}", target_path.display()))?;
        if closure_paths.insert(target_path.clone()) {
            closure_files.push((target_path.clone(), src_text.into_bytes()));
        }
        let target_dir = target_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| base_dir.to_path_buf());

        let mut lib_section: Vec<Stmt> = ast.stmts.drain(lib_offset..).collect();

        // §16.2.1.10 ns `default` field — see `default_binding.rs`.
        // Not on a star pour: §16.2.3 `export * from` never forwards
        // `default`, so a star-fed walk of a default-carrying module
        // must not mint the field.
        let repeat_default_field = if ns_star_feed {
            None
        } else {
            default_binding::synth_ns_default_binding(
                &lib_section,
                &namespace_alias,
                &mut default_alias,
                &default_bound_as,
                &target_path,
            )
        };
        if let Some(a) = &default_alias {
            default_bound_as.insert(target_path.clone(), a.clone());
        }
        let (want, rename, bare_exports, own_exports, demangle) = deconflict::prep_lib_request(
            ast,
            &mut lib_section,
            lib_expr_offset,
            &target_path,
            &named,
            &mut seen_names,
            mangled_by_path.entry(target_path.clone()).or_default(),
            &mut mangle_seq,
        )?;

        // For P13-S2 namespace import: the accumulator collecting the
        // names of every value-export injected (FnDecl / LetDecl /
        // ClassDecl). `first_field` marks where THIS lib's own
        // contribution starts, so the dyn-ns revisit shortcut still
        // records per-path fields.
        if let Some(alias) = &namespace_alias
            && !ns_accums.contains_key(alias)
        {
            ns_order.push(alias.clone());
            ns_accums.insert(alias.clone(), NsAccum::new(alias.clone()));
        }
        let first_field = namespace_alias
            .as_ref()
            .map(|a| ns_accums[a].fields().len())
            .unwrap_or(0);
        let mut req = LibRequest {
            side_effect_only,
            default_alias: &default_alias,
            want: &want,
            rename: &rename,
            ns: namespace_alias
                .as_ref()
                .and_then(|a| ns_accums.get_mut(a.as_str())),
            bare_exports: &bare_exports,
            own_exports: &own_exports,
            // Re-borrowed rather than kept from the filter above —
            // the parse in between needed `ast`, and the walk is the
            // ledger's writer: every decl it injects records here so
            // a later request (any lane) skips it.
            injected: injected_names.entry(target_path.clone()).or_default(),
            demangle: &demangle,
        };

        let mut injections = injections_by_path.remove(&target_path).unwrap_or_default();
        let work_mark = work.len();
        for s in lib_section {
            walk_lib_stmt(ast, &mut work, &target_dir, &mut injections, s, &mut req)?;
        }

        let deps = graph.edges_since(&work, work_mark);
        graph.record(target_path.clone(), deps);
        injections_by_path.insert(target_path.clone(), injections);

        // The repeat half of the `default` field — the binding already
        // exists, only the field claim is owed. Before the ns_fields
        // snapshot so a dyn revisit carries it.
        if let Some(binding) = repeat_default_field
            && let Some(alias) = &namespace_alias
            && let Some(accum) = ns_accums.get_mut(alias.as_str())
        {
            accum.claim("default", &binding);
        }
        if let Some(alias) = &namespace_alias {
            let fields = ns_accums[alias].fields()[first_field..].to_vec();
            ns_fields_by_path.insert(target_path.clone(), fields);
        }
    }

    let mut injections: Vec<Stmt> = Vec::new();
    for path in graph.evaluation_order() {
        if let Some(stmts) = injections_by_path.remove(&path) {
            injections.extend(stmts);
        }
    }
    // Dispatcher before `inline_dyn_ns_objlits` — see `synth_dispatcher`.
    if ast.dyn_import_present {
        injections.push(dyn_import::synth_dispatcher(ast, &dyn_table));
    }
    // The synthetic `let ns = { … }` bindings only reference decls, so
    // they land after every module's statements regardless of which
    // module asked for them.
    materialize_pending_namespaces(
        ast,
        &mut injections,
        &ns_order,
        &ns_accums,
        &mut dyn_ns_inline,
    );
    inline_dyn_ns_objlits(ast, &dyn_ns_inline);

    splice_injections(ast, injections);
    Ok(closure_files)
}

mod deconflict;
mod default_binding;
mod dyn_import;
mod graph;
mod inject_export;
mod lib_walk;
mod resolve_helpers;
mod splice;
use graph::ModuleGraph;
use inject_export::{inject_bare_exported_decl, inject_export_inner};
use lib_walk::{LibRequest, walk_lib_stmt};
use resolve_helpers::{
    NsAccum, filter_request, inline_dyn_ns_objlits, materialize_pending_namespaces,
    queue_nested_import, queue_reexport, queue_star_reexport,
};
use splice::splice_injections;

fn check_k2_form(
    _source: &str,
    _default: &Option<String>,
    _namespace: &Option<String>,
    _named: &[(String, Option<String>)],
) -> Result<(), String> {
    // All four import forms (named / default / namespace / side-effect)
    // are now lifted through P13-S1/S2/S3. Bare named exports `export { a }
    // from "./b"` (re-export, no inner) remain rejected at the lib walk
    // (the P13-S4 surface); other rejects live at parse sites.
    Ok(())
}

/// True for module sources that map to runtime built-ins instead of
/// user files. Mirrored by `ast::desugar_builtin_imports`. Keep the
/// two lists in sync.
fn is_builtin_module_source(source: &str) -> bool {
    matches!(
        source,
        "fs" | "node:fs" | "fs/promises" | "node:fs/promises"
    )
}

/// Resolve a relative or absolute import path against the importer's
/// directory. Tries the literal path first, then falls back to a `.ts`
/// suffix if the literal isn't a file (`./lib` → `./lib.ts`). Always
/// canonicalizes — the canonical form is the visited-set key, so two
/// different relative spellings of the same file resolve to the same
/// node and the cycle check fires once.
fn resolve_path(base_dir: &Path, source: &str) -> Result<PathBuf, String> {
    let candidate = if Path::new(source).is_absolute() {
        PathBuf::from(source)
    } else {
        base_dir.join(source)
    };
    let final_path = if candidate.is_file() {
        candidate
    } else {
        let with_ts = candidate.with_extension("ts");
        if with_ts.is_file() {
            with_ts
        } else {
            return Err(format!(
                "import path not found: {source} (resolved against {})",
                base_dir.display()
            ));
        }
    };
    final_path
        .canonicalize()
        .map_err(|e| format!("canonicalize {}: {e}", final_path.display()))
}

fn decl_name(s: &Stmt) -> Option<String> {
    match s {
        Stmt::FnDecl { name, .. }
        | Stmt::LetDecl { name, .. }
        | Stmt::TypeDecl { name, .. }
        | Stmt::ClassDecl { name, .. } => Some(name.clone()),
        _ => None,
    }
}

fn rename_decl(s: &mut Stmt, new_name: String) {
    match s {
        Stmt::FnDecl { name, .. }
        | Stmt::LetDecl { name, .. }
        | Stmt::TypeDecl { name, .. }
        | Stmt::ClassDecl { name, .. } => *name = new_name,
        _ => {}
    }
}
