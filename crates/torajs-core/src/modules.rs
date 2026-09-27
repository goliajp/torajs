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

use crate::ast::Ast;
use std::collections::VecDeque;
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

/// Seed every root request onto the worklist: the entry's static
/// clauses (`static_resolution.rs`), then §13.3.10 dynamic-import
/// candidates — queued before `graph.roots` records so their
/// injections ride the dependency-order splice.
fn seed_requests(
    ast: &mut Ast,
    base_dir: &Path,
    entry: Option<&Path>,
    work: &mut VecDeque<WorkItem>,
    graph: &mut ModuleGraph,
) -> Result<(static_resolution::EntrySeed, dyn_import::DynTable), String> {
    let mut entry_seed = static_resolution::seed_entry_requests(ast, base_dir, work)?;
    entry_seed.entry_path = entry.and_then(|p| p.canonicalize().ok());
    entry_seed.static_roots = graph.edges_since(work, 0);
    let static_len = work.len();
    let dyn_table = dyn_import::seed_candidates(ast, base_dir, work);
    entry_seed.dyn_roots = work
        .iter()
        .skip(static_len)
        .filter_map(|(path, _, _, _, ns, _)| ns.as_ref().map(|n| (path.clone(), n.clone())))
        .collect();
    graph.roots = graph.edges_since(work, 0);
    Ok((entry_seed, dyn_table))
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
///
/// `entry` is the entry file itself when it has one (stdin has none):
/// a request that names it on the side-effect lane (`import
/// "./self"`, or a lib's `export { x } from "./entry"`) evaluates
/// nothing extra — the entry IS the program — where a lib walk would
/// splice a second copy of its whole body in front of it.
pub fn resolve_imports(
    ast: &mut Ast,
    base_dir: &Path,
    entry: Option<&Path>,
) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let mut st = resolve_state::ResolveState::new(ast, base_dir);
    let (entry_seed, dyn_table) = seed_requests(ast, base_dir, entry, &mut st.work, &mut st.graph)?;

    while let Some((
        target_path,
        named,
        default_alias,
        side_effect_only,
        namespace_alias,
        ns_star_feed,
    )) = pop_next(&mut st.work)
    {
        // Repeat-visit alignment + per-path filter + mangle-memory
        // realignment — see [`repeat_request::align_and_filter`].
        let Some((named, mut default_alias, namespace_alias, side_effect_only)) =
            repeat_request::align_and_filter(
                ast,
                &target_path,
                entry_seed.entry_path.as_deref(),
                named,
                default_alias,
                namespace_alias,
                side_effect_only,
                &st.ns.fields_by_path,
                &mut st.ns.dyn_ns_repeat,
                &st.default_bound_as,
                &mut st.injections_by_path,
                &mut st.injected_names,
                &st.mangled_by_path,
            )
        else {
            continue;
        };

        let (mut lib_section, lib_expr_offset, target_dir, table_delta) = load_lib_section(
            ast,
            &target_path,
            base_dir,
            &mut st.closure_paths,
            &mut st.closure_files,
        )?;

        // §16.2.1.10 ns `default` field — see `default_binding.rs`.
        let repeat_default_field = default_binding::settle_walk_default(
            &lib_section,
            &namespace_alias,
            &mut default_alias,
            &mut st.default_bound_as,
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
                &mut st.seen_names,
                st.mangled_by_path.entry(target_path.clone()).or_default(),
                st.hidden_by_path.entry(target_path.clone()).or_default(),
                &mut st.mangle_seq,
                deconflict::LaneShape {
                    has_default_alias: default_alias.is_some(),
                    is_ns: namespace_alias.is_some(),
                    side_effect_only,
                },
                &table_delta,
            )?;

        let first_field =
            repeat_request::open_ns_walk(&namespace_alias, &mut st.ns.accums, &mut st.ns.order);
        let mut req = LibRequest {
            side_effect_only,
            default_alias: &default_alias,
            want: &want,
            rename: &rename,
            ns: namespace_alias
                .as_ref()
                .and_then(|a| st.ns.accums.get_mut(a.as_str())),
            bare_exports: &bare_exports,
            own_exports: &own_exports,
            // Re-borrowed rather than kept from the filter above —
            // the parse in between needed `ast`, and the walk is the
            // ledger's writer: every decl it injects records here so
            // a later request (any lane) skips it.
            injected: st.injected_names.entry(target_path.clone()).or_default(),
            demangle: &demangle,
            hidden: &hidden,
            landings: inject_export::Landings {
                path: &target_path,
                first: &mut st.landing_first,
                ambiguous: &mut st.ambiguous_locals,
            },
            ledger: st.ledger.ledger(&target_path),
            // A candidate that is the entry itself has no body to run at
            // its import: the entry IS the program, already evaluating
            // (§16.2.1.5.3 — an `import()` of a module in that status
            // hands back its namespace without a second evaluation).
            deferred: namespace_alias
                .as_ref()
                .filter(|a| entry_seed.dyn_roots.iter().any(|(_, n)| n == *a))
                .filter(|_| target_path.canonicalize().ok() != entry_seed.entry_path)
                .map(|a| st.dyn_eval_bodies.entry(a.clone()).or_default()),
        };

        let mut injections = st
            .injections_by_path
            .remove(&target_path)
            .unwrap_or_default();
        let work_mark = st.work.len();
        for s in lib_section {
            walk_lib_stmt(ast, &mut st.work, &target_dir, &mut injections, s, &mut req)?;
        }

        st.graph.record(
            target_path.clone(),
            st.graph.edges_since(&st.work, work_mark),
        );
        st.injections_by_path
            .insert(target_path.clone(), injections);
        repeat_request::record_ns_walk(
            &namespace_alias,
            repeat_default_field,
            &mut st.ns.accums,
            first_field,
            &target_path,
            &mut st.ns.fields_by_path,
        );
    }

    // §16.2.1.5/.6 post-BFS judgment + final assembly + dead-copy
    // sweep — see `splice::settle_and_splice`.
    splice::settle_and_splice(
        ast,
        &st.graph,
        st.injections_by_path,
        &dyn_table,
        &st.ns.order,
        &st.ns.accums,
        &mut st.ns.dyn_ns_repeat,
        &st.ambiguous_locals,
        &entry_seed,
        &st.ledger.nested,
        &st.dyn_eval_bodies,
    )?;
    Ok(st.closure_files)
}

mod dead_lib_exprs;
mod deconflict;
mod default_binding;
mod dyn_import;
mod dyn_import_dispatcher;
mod graph;
mod inject_export;
mod lib_walk;
mod load_lib;
mod namespace_objlit;
mod ns_tables;
mod repeat_request;
mod resolve_helpers;
mod resolve_state;
pub(crate) mod splice;
mod static_resolution;
use graph::ModuleGraph;
use inject_export::{decl_name, inject_bare_exported_decl, inject_export_inner};
use lib_walk::{LibRequest, walk_lib_stmt};
use load_lib::{load_lib_section, pop_next};
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
