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

// Entry-request seeding + post-BFS static-resolution judgment — see
// `static_resolution.rs`.

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
    // Knife C hidden-dep mangles keep their own per-path memory —
    // NOT `mangled_by_path`: a later plain named request for a
    // hidden-mangled export must re-inject bare, not trip the
    // requested-collision reject (see `deconflict_lib_section`).
    let mut hidden_by_path: HashMap<PathBuf, HashMap<String, String>> = HashMap::new();
    let mut mangle_seq: usize = 0;

    // §16.2.1.5 — the arena length BEFORE any lib parses marks which
    // expressions the entry itself wrote (import-binding write
    // rejection scopes to exactly those).
    let entry_expr_len = ast.exprs.len();
    let (static_requests, import_bindings) =
        static_resolution::seed_entry_requests(ast, base_dir, &mut work)?;
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
    // §16.2.1.6.3 ambiguity ledger — see `inject_export::Landings`.
    let mut landing_first: HashMap<String, PathBuf> = HashMap::new();
    let mut ambiguous_locals: HashSet<String> = HashSet::new();
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
        default_alias,
        side_effect_only,
        namespace_alias,
        ns_star_feed,
    )) = work.pop_front()
    {
        // Repeat-visit alignment + per-path filter + mangle-memory
        // realignment — see [`repeat_request::align_and_filter`].
        let Some((named, mut default_alias, namespace_alias, side_effect_only)) =
            repeat_request::align_and_filter(
                ast,
                &target_path,
                named,
                default_alias,
                namespace_alias,
                side_effect_only,
                &ns_fields_by_path,
                &mut dyn_ns_inline,
                &default_bound_as,
                &mut injections_by_path,
                &mut injected_names,
                &mangled_by_path,
            )
        else {
            continue;
        };

        let (mut lib_section, lib_expr_offset, target_dir, table_delta) = load_lib_section(
            ast,
            &target_path,
            base_dir,
            &mut closure_paths,
            &mut closure_files,
        )?;

        // §16.2.1.10 ns `default` field — see `default_binding.rs`.
        let repeat_default_field = default_binding::settle_walk_default(
            &lib_section,
            &namespace_alias,
            &mut default_alias,
            &mut default_bound_as,
            &target_path,
            ns_star_feed,
        );
        let (want, rename, bare_exports, own_exports, demangle, hidden) =
            deconflict::prep_lib_request(
                ast,
                &mut lib_section,
                lib_expr_offset,
                &target_path,
                &named,
                &mut seen_names,
                mangled_by_path.entry(target_path.clone()).or_default(),
                hidden_by_path.entry(target_path.clone()).or_default(),
                &mut mangle_seq,
                deconflict::LaneShape {
                    has_default_alias: default_alias.is_some(),
                    is_ns: namespace_alias.is_some(),
                    side_effect_only,
                },
                &table_delta,
            )?;

        let first_field =
            repeat_request::open_ns_walk(&namespace_alias, &mut ns_accums, &mut ns_order);
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
            hidden: &hidden,
            landings: inject_export::Landings {
                path: &target_path,
                first: &mut landing_first,
                ambiguous: &mut ambiguous_locals,
            },
        };

        let mut injections = injections_by_path.remove(&target_path).unwrap_or_default();
        let work_mark = work.len();
        for s in lib_section {
            walk_lib_stmt(ast, &mut work, &target_dir, &mut injections, s, &mut req)?;
        }

        let deps = graph.edges_since(&work, work_mark);
        graph.record(target_path.clone(), deps);
        injections_by_path.insert(target_path.clone(), injections);

        repeat_request::record_ns_walk(
            &namespace_alias,
            repeat_default_field,
            &mut ns_accums,
            first_field,
            &target_path,
            &mut ns_fields_by_path,
        );
    }

    // §16.2.1.6.2/.3 — the entry's static requests must all have
    // resolved (see `static_resolution.rs`); the dyn-import lane
    // keeps its own per-candidate promise-reject poisoning.
    static_resolution::check(
        &static_requests,
        &injections_by_path,
        &ns_accums,
        &ambiguous_locals,
    )?;
    static_resolution::reject_import_binding_writes(ast, &import_bindings, entry_expr_len);

    splice::assemble_and_splice(
        ast,
        &graph,
        injections_by_path,
        &dyn_table,
        &ns_order,
        &ns_accums,
        &mut dyn_ns_inline,
        &ambiguous_locals,
    );
    Ok(closure_files)
}

/// Read, tokenize, and parse one module file into the shared arena,
/// recording it for the compile-closure cache key; answers the
/// drained lib section, the arena offset its expressions start at,
/// and the directory its own imports resolve against.
fn load_lib_section(
    ast: &mut Ast,
    target_path: &Path,
    base_dir: &Path,
    closure_paths: &mut HashSet<PathBuf>,
    closure_files: &mut Vec<(PathBuf, Vec<u8>)>,
) -> Result<(Vec<Stmt>, usize, PathBuf, deconflict::LibTableDelta), String> {
    let src_text = std::fs::read_to_string(target_path)
        .map_err(|e| format!("import {}: {e}", target_path.display()))?;
    let tokens = lexer::tokenize(&src_text)
        .map_err(|e| format!("import {} lex: {e}", target_path.display()))?;
    let lib_expr_offset = ast.exprs.len();
    // Knife D — snapshot the name-keyed class tables around the lib's
    // parse: the diff tells the census which rows THIS file wrote, so
    // a class rename can split rows that a same-named entry class
    // merged into (see `class_rename`'s module doc).
    let table_snap = deconflict::snapshot_class_tables(ast);
    let lib_offset = parser::parse_into(&src_text, &tokens, ast)
        .map_err(|e| format!("import {} parse: {e}", target_path.display()))?;
    let table_delta = deconflict::diff_class_tables(ast, &table_snap);
    // A lib expression's recorded span indexes the LIB file's text,
    // but every span consumer slices the MAIN file's `ast.source`
    // (`splice_injections` clears the Stmt-side spans for the same
    // reason). The arrow / fn-expr spans ride per-ExprId, and a
    // lifted closure decl inherits its arrow's span (B1b) — reset
    // the lib slice to the (0,0) "no user source" sentinel so
    // `intern_fn_source` answers the native form instead of slicing
    // out of bounds (or silently wrong text) on the entry's source.
    for sp in ast.expr_spans[lib_expr_offset..].iter_mut() {
        *sp = lexer::Span { start: 0, end: 0 };
    }
    if closure_paths.insert(target_path.to_path_buf()) {
        closure_files.push((target_path.to_path_buf(), src_text.into_bytes()));
    }
    let target_dir = target_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| base_dir.to_path_buf());
    let lib_section: Vec<Stmt> = ast.stmts.drain(lib_offset..).collect();
    Ok((lib_section, lib_expr_offset, target_dir, table_delta))
}

mod deconflict;
mod default_binding;
mod dyn_import;
mod graph;
mod inject_export;
mod lib_walk;
mod repeat_request;
mod resolve_helpers;
mod splice;
mod static_resolution;
use graph::ModuleGraph;
use inject_export::{decl_name, inject_bare_exported_decl, inject_export_inner};
use lib_walk::{LibRequest, walk_lib_stmt};
use resolve_helpers::{NsAccum, queue_reexport, queue_star_reexport};

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
