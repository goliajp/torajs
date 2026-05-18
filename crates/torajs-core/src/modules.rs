//! Phase K.2 — cross-file imports.
//!
//! Walks the main file's `ImportDecl`s, reads the targeted files, parses
//! them into the same `Ast` arena via `parser::parse_into` (so ExprIds
//! stay coherent), and prepends each imported file's requested
//! `export`-wrapped declarations to the main file's stmts. Imports are
//! resolved breadth-first; the visited set keys on the canonicalized
//! absolute path so cyclic imports terminate naturally.
//!
//! Subset boundary (will lift in later phases):
//!   - `import { a, b as c } from "./y"`     — supported
//!   - `import x from "./y"`                  — rejected (default)
//!   - `import * as ns from "./y"`            — rejected (namespace)
//!   - `import "./y"`                         — rejected (side-effect)
//!   - `export { a, b }` (re-export, no inner) — rejected
//!
//! Behaviour notes:
//!   - Lib-level non-`export` top-level statements are silently dropped.
//!     A lib is treated as a pure declaration source — its `console.log`
//!     etc. at top-level do NOT run at import time. This is a documented
//!     subset deviation from bun's runtime-evaluation semantics; bench
//!     and conformance avoid relying on it.
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
/// the request.
type WorkItem = (PathBuf, Vec<NamedImport>);

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
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut work: VecDeque<WorkItem> = VecDeque::new();

    for s in &ast.stmts {
        if let Stmt::ImportDecl {
            source,
            named,
            default,
            namespace,
        } = s
        {
            /* Built-in modules (`fs`, `node:fs`, ...) skip filesystem
             * resolution — `desugar_builtin_imports` rewrites their
             * imported names into namespace-method calls before
             * downstream passes run. */
            if is_builtin_module_source(source) {
                continue;
            }
            check_k2_form(source, default, namespace, named)?;
            let path = resolve_path(base_dir, source)?;
            work.push_back((path, named.clone()));
        }
    }

    let mut injections: Vec<Stmt> = Vec::new();

    while let Some((target_path, named)) = work.pop_front() {
        if !visited.insert(target_path.clone()) {
            continue;
        }
        let src_text = std::fs::read_to_string(&target_path)
            .map_err(|e| format!("import {}: {e}", target_path.display()))?;
        let tokens = lexer::tokenize(&src_text)
            .map_err(|e| format!("import {} lex: {e}", target_path.display()))?;
        let lib_offset = parser::parse_into(&tokens, ast)
            .map_err(|e| format!("import {} parse: {e}", target_path.display()))?;
        closure_files.push((target_path.clone(), src_text.into_bytes()));
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

        for s in lib_section {
            match s {
                Stmt::ImportDecl {
                    source,
                    named,
                    default,
                    namespace,
                } => {
                    if is_builtin_module_source(&source) {
                        continue;
                    }
                    check_k2_form(&source, &default, &namespace, &named)?;
                    let path = resolve_path(&target_dir, &source)?;
                    work.push_back((path, named));
                }
                Stmt::ExportDecl {
                    inner: Some(boxed), ..
                } => {
                    let mut inner = *boxed;
                    // Type decls always inject — TS doesn't require type
                    // names in the value-import list, and downstream
                    // check.rs needs them to resolve fn return-type
                    // annotations on imported value decls.
                    let always_inject = matches!(inner, Stmt::TypeDecl { .. });
                    if always_inject {
                        injections.push(inner);
                        continue;
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
                Stmt::ExportDecl { inner: None, .. } => {
                    return Err(format!(
                        "bare named export not supported in K.2 ({})",
                        target_path.display()
                    ));
                }
                _ => {
                    // Lib-level non-export top-level stmt — dropped.
                }
            }
        }
    }

    if !injections.is_empty() {
        let mut new_stmts = injections;
        new_stmts.extend(std::mem::take(&mut ast.stmts));
        ast.stmts = new_stmts;
    }
    Ok(closure_files)
}

fn check_k2_form(
    source: &str,
    default: &Option<String>,
    namespace: &Option<String>,
    named: &[(String, Option<String>)],
) -> Result<(), String> {
    if default.is_some() {
        return Err(format!(
            "default import (`import x from \"{source}\"`) not supported in K.2"
        ));
    }
    if namespace.is_some() {
        return Err(format!(
            "namespace import (`import * as ns from \"{source}\"`) not supported in K.2"
        ));
    }
    if named.is_empty() {
        return Err(format!(
            "side-effect-only import (`import \"{source}\"`) not supported in K.2"
        ));
    }
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
