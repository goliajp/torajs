//! Phase K.2 — cross-file imports.
//!
//! Walks the main file's `ImportDecl`s, reads the targeted files, parses
//! them into the same `Ast` arena via `parser::parse_into` (so ExprIds
//! stay coherent), and prepends each imported file's requested
//! `export`-wrapped declarations to the main file's stmts. Imports are
//! resolved breadth-first; the visited set keys on the canonicalized
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
//!   - `as <alias>` flat-renames the injected decl. Recursive references
//!     inside the imported decl still bind to the original name — if
//!     the importer renames `foo` to `bar`, `foo`'s recursive call to
//!     itself looks up `foo`, which is no longer in scope. This is a
//!     known K.2 corner; revisit if it bites a real use case.

use crate::ast::{Ast, Stmt};
use crate::lexer;
use crate::parser;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

/// One named entry in an `import` clause: `(orig_name, alias)`. `alias`
/// is `Some` for `import { foo as bar }`, `None` otherwise.
type NamedImport = (String, Option<String>);

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
type WorkItem = (
    PathBuf,
    Vec<NamedImport>,
    Option<String>,
    bool,
    Option<String>,
);

/// One statement of a side-effect-only (`import "./x"`) lib walk —
/// the lib's ImportDecls still queue (so transitive loads happen),
/// but every other top-level statement (including bare expressions,
/// console.log, top-level `let`s, classes, and `export`-wrapped
/// decls) injects in source order. Bare named export (`export
/// { a }`, the P13-S4 re-export surface) is still rejected at the
/// side-effect boundary (K.2).
fn inject_side_effect_stmt(
    work: &mut VecDeque<WorkItem>,
    target_dir: &Path,
    injections: &mut Vec<Stmt>,
    s: Stmt,
) -> Result<(), String> {
    if let Stmt::ImportDecl {
        source,
        named,
        default,
        namespace,
    } = s
    {
        queue_nested_import(work, target_dir, &source, named, default, namespace)?;
        return Ok(());
    }
    if let Stmt::ExportDecl {
        inner: Some(boxed), ..
    } = s
    {
        injections.push(*boxed);
    } else if let Stmt::ExportDecl { inner: None, .. } = s {
        // P13-S4b — a side-effect import binds no names, so the
        // export FACE of a bare named export (`export { a }`) has no
        // consumer here; the decls it lists already injected as
        // ordinary top-level statements above. Drop the face.
    } else {
        injections.push(s);
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

    for s in &ast.stmts {
        if let Stmt::ImportDecl {
            source,
            named,
            default,
            namespace,
        } = s
        {
            queue_nested_import(
                &mut work,
                base_dir,
                source,
                named.clone(),
                default.clone(),
                namespace.clone(),
            )?;
        }
    }

    let mut injections: Vec<Stmt> = Vec::new();

    while let Some((target_path, named, default_alias, side_effect_only, namespace_alias)) =
        work.pop_front()
    {
        // Filter the request against per-path injected state — see
        // [`filter_request`].
        let entry = injected_names.entry(target_path.clone()).or_default();
        let Some((named, default_alias, namespace_alias, side_effect_only)) = filter_request(
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
        let lib_offset = parser::parse_into(&src_text, &tokens, ast)
            .map_err(|e| format!("import {} parse: {e}", target_path.display()))?;
        if closure_paths.insert(target_path.clone()) {
            closure_files.push((target_path.clone(), src_text.into_bytes()));
        }
        let target_dir = target_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| base_dir.to_path_buf());

        let lib_section: Vec<Stmt> = ast.stmts.drain(lib_offset..).collect();

        let want: HashSet<&str> = named.iter().map(|(n, _)| n.as_str()).collect();
        let rename: HashMap<&str, &str> = named
            .iter()
            .filter_map(|(orig, alias)| alias.as_deref().map(|a| (orig.as_str(), a)))
            .collect();
        // For P13-S2 namespace import: accumulates the names of every
        // value-export injected (FnDecl / LetDecl / ClassDecl). After
        // the walk the namespace alias materializes as a synthetic
        // `let <alias> = { name1: name1, name2: name2, ... }`.
        let mut namespace_fields: Vec<String> = Vec::new();

        let bare_exports = collect_bare_exports(&lib_section);

        for s in lib_section {
            // Side-effect-only import — see [`inject_side_effect_stmt`].
            if side_effect_only {
                inject_side_effect_stmt(&mut work, &target_dir, &mut injections, s)?;
                continue;
            }
            match s {
                Stmt::ImportDecl {
                    source,
                    named,
                    default,
                    namespace,
                } => {
                    queue_nested_import(
                        &mut work,
                        &target_dir,
                        &source,
                        named,
                        default,
                        namespace,
                    )?;
                }
                Stmt::ExportDecl {
                    inner: None,
                    default_expr: Some(eid),
                    ..
                } => {
                    // `export default <expr>` form. Inject as a synthetic
                    // `let <importer-alias> = <expr>` if the importer used
                    // the default-binding form (`import x from ...`).
                    if let Some(alias) = &default_alias {
                        injections.push(Stmt::LetDecl {
                            mutable: false,
                            name: alias.clone(),
                            type_ann: None,
                            init: eid,
                            is_var: false,
                        });
                    }
                    // If no default alias was requested the export is
                    // silently dropped — matches "named exports not in
                    // the importer's list" behavior for parity.
                }
                Stmt::ExportDecl {
                    inner: Some(boxed), ..
                } => {
                    inject_export_inner(
                        &mut injections,
                        *boxed,
                        &want,
                        &rename,
                        namespace_alias.is_some(),
                        &mut namespace_fields,
                    );
                }
                Stmt::ExportDecl {
                    inner: None,
                    named: lib_named,
                    source: Some(lib_source),
                    ..
                } => {
                    queue_reexport(
                        &mut work,
                        &target_dir,
                        &lib_named,
                        &lib_source,
                        &want,
                        &rename,
                    )?;
                }
                Stmt::ExportDecl { inner: None, .. } => {
                    // P13-S4b — the bare named export FACE: its
                    // (orig, exported) pairs were collected before
                    // the walk; the decls themselves inject below.
                }
                other => {
                    inject_bare_exported_decl(
                        &mut injections,
                        other,
                        &bare_exports,
                        &want,
                        &rename,
                        namespace_alias.is_some(),
                        &mut namespace_fields,
                    );
                }
            }
        }

        if let Some(ns_name) = namespace_alias {
            materialize_namespace(ast, &mut injections, ns_name, &namespace_fields);
        }
    }

    if !injections.is_empty() {
        let mut new_stmts = injections;
        // An injected lib decl's recorded span indexes the LIB
        // file's text, but every span consumer downstream
        // (`intern_fn_source` / the class-method registry) slices
        // the MAIN file's `ast.source`: out of bounds when the main
        // file is shorter, silently wrong toString text otherwise.
        // Reset to the (0,0) "no user source" sentinel — toString
        // answers the native form.
        for s in &mut new_stmts {
            clear_injected_spans(s);
        }
        new_stmts.extend(std::mem::take(&mut ast.stmts));
        ast.stmts = new_stmts;
    }
    Ok(closure_files)
}

/// P13-S4b — bare named exports (`export { a, b as c }`, no `from`)
/// alias top-level decls declared elsewhere in the lib. Collected
/// orig → exported up front so the decl statements (previously
/// dropped as non-exports) inject when the importer wants them.
fn collect_bare_exports(lib_section: &[Stmt]) -> HashMap<String, String> {
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
fn inject_bare_exported_decl(
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

/// See the injection-splice comment in [`resolve_imports`] — recurse
/// through nested fn bodies so every registry-visible declaration
/// carries the sentinel.
fn clear_injected_spans(s: &mut Stmt) {
    match s {
        Stmt::FnDecl { span, body, .. } => {
            *span = crate::lexer::Span { start: 0, end: 0 };
            for inner in body {
                clear_injected_spans(inner);
            }
        }
        Stmt::ClassDecl {
            methods,
            static_methods,
            ..
        } => {
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                m.span = crate::lexer::Span { start: 0, end: 0 };
            }
        }
        _ => {}
    }
}

mod resolve_helpers;
use resolve_helpers::{
    filter_request, inject_export_inner, materialize_namespace, queue_nested_import, queue_reexport,
};

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
