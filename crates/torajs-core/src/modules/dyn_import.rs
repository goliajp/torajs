//! Dynamic-import (§13.3.10) candidate collection + dispatcher
//! synthesis.
//!
//! Every `import(<expr>)` call site — string literal or not — parses
//! to `__torajs_dyn_import(<expr>)` (see
//! `parser/primary_atoms.rs::parse_primary_dyn_import`). This module
//! gives that name a body:
//!
//! 1. [`collect_candidates`] — the AOT candidate table, webpack
//!    context-module style narrowed to EXACT literals: every string
//!    literal in the entry file that resolves to a module file is a
//!    candidate specifier. The resolver bakes each candidate through
//!    the same namespace-injection lane the static `import * as ns`
//!    form uses (`__dyn_ns_c<k>` aliases bind the same synthetic
//!    top-level `let`).
//! 2. `super::dyn_import_dispatcher::synth_dispatcher` — the runtime
//!    half, per §13.3.10 evaluation:
//!    ToString(specifier) with an abrupt completion rejecting the
//!    promise (step 7 IfAbruptRejectPromise), an exact-match table
//!    probe, and a miss rejecting with a TypeError.
//!
//! Recorded subset boundaries (all LOUD or reject-at-runtime, never
//! silent-wrong):
//! - Candidates are EAGER: a baked module's top-level bindings load
//!   whether or not the `import()` ever runs — same posture as the
//!   pre-existing eager literal form.
//! - A specifier string built at runtime that never appears as one
//!   literal in the source (`'./a' + '_b.js'` concatenation) is not
//!   in the table — the call rejects. Prefix-directory scanning
//!   (webpack's ContextModule) is the follow-up that would widen this.
//! - A candidate whose UNMANGLABLE names (see [`unmanglable_names`])
//!   collide with the entry's own top-level names, its imports, or an
//!   earlier candidate's injected face is DROPPED (rejects at
//!   runtime): the 423-01 deconflict census renames colliding
//!   unrequested Fn/Let decls at walk time, but a name the census
//!   cannot rename would redeclare — and a speculative candidate must
//!   never turn a valid program into a compile error. A collision
//!   inside a candidate's TRANSITIVE imports is outside this check
//!   (same boundary as before: only the candidate's own file is
//!   speculatively parsed).

use crate::ast::{Ast, Expr, Stmt};
use crate::lexer;
use crate::parser;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use super::deconflict::{
    is_class_decl, rebinds_elsewhere, top_value_decl_name, type_param_shadows,
};
use super::export_faces::{collect_bare_exports, collect_own_export_names};
use super::{WorkItem, decl_name, resolve_path};

/// The dispatcher FnDecl's name. Parser-minted at every `import()`
/// call site; no user identifier can collide (double-underscore
/// reserve).
pub(super) const DISPATCHER_NAME: &str = "__torajs_dyn_import";

/// What the dispatcher answers for one candidate specifier.
pub(super) enum DynTarget {
    /// The candidate loaded: resolve with its `__dyn_ns_c<k>`
    /// namespace (unless the link poisoned it — `poisoned_candidates`).
    Namespace(String),
    /// The file exists but is not module code tr compiles — ParseModule
    /// failed (§16.2.1.6 HostLoadImportedModule; a lex / parse error or
    /// a §16.2.1.1 top-level early error): reject with a SyntaxError
    /// carrying the reason. It is never queued, so nothing of it
    /// injects.
    SyntaxError(String),
}

/// The `(specifier literal, target)` table the dispatcher matches
/// against.
pub(super) type DynTable = Vec<(String, DynTarget)>;

/// Queue every candidate module onto the BFS worklist under a
/// `__dyn_ns_c<k>` namespace alias (bound as a top-level `let` like a
/// static `import * as ns`) and answer the
/// `(specifier literal, alias)` table the dispatcher matches against.
/// No-op (empty table) when the entry has no dynamic import.
pub(super) fn seed_candidates(
    ast: &Ast,
    base_dir: &Path,
    work: &mut VecDeque<WorkItem>,
) -> DynTable {
    let mut table: DynTable = Vec::new();
    if ast.dyn_import_present {
        for (k, (lit, probe)) in collect_candidates(ast, base_dir).into_iter().enumerate() {
            let target = match probe {
                Ok(path) => {
                    let ns_name = format!("__dyn_ns_c{k}");
                    work.push_back(WorkItem::namespace(path, ns_name.clone()));
                    DynTarget::Namespace(ns_name)
                }
                Err(msg) => {
                    DynTarget::SyntaxError(format!("module '{lit}' failed to parse: {msg}"))
                }
            };
            table.push((lit, target));
        }
    }
    table
}

/// Scan the entry file's expression arena for string literals that
/// resolve to loadable module files. Must run BEFORE the BFS pops any
/// lib (the arena then only holds the entry's expressions).
///
/// A candidate is accepted when:
/// - the literal is relative (`./` / `../`) — bare names are package
///   specifiers, absolute paths are not module code we bake;
/// - it resolves to an existing file ([`resolve_path`], which also
///   tries the `.ts` suffix fallback);
/// - a speculative parse succeeds (the file may be any bytes — a
///   candidate must not turn the build into a parse error). A file
///   that lexes / parses wrong, or trips a module top-level early
///   error, is still a candidate: one that REJECTS with a SyntaxError
///   (`Err(reason)` here, `DynTarget::SyntaxError` in the table) —
///   §16.2.1.6 HostLoadImportedModule's failed ParseModule, which a
///   miss's TypeError would misname (test262 `eval-script-code-
///   target`: a Script-only body imported as a module);
/// - its UNMANGLABLE names don't collide with the entry's reserved
///   names or an earlier candidate's injected face (see module doc);
///   every other collision deconflicts at walk time.
fn collect_candidates(ast: &Ast, base_dir: &Path) -> Vec<(String, Result<PathBuf, String>)> {
    let mut reserved = entry_reserved_names(ast, base_dir);
    let mut seen_lits: HashSet<&str> = HashSet::new();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let mut out: Vec<(String, Result<PathBuf, String>)> = Vec::new();
    for e in &ast.exprs {
        let Expr::String(lit) = e else { continue };
        // A specifier with a lone surrogate names no file.
        let Some(lit) = lit.as_str() else { continue };
        if !(lit.starts_with("./") || lit.starts_with("../")) {
            continue;
        }
        if !seen_lits.insert(lit) {
            continue;
        }
        let Ok(path) = resolve_path(base_dir, lit) else {
            continue;
        };
        if seen_paths.contains(&path) {
            // Two spellings of one file: the first literal claimed it.
            continue;
        }
        let probe = match speculative_probe(&path) {
            Some(Ok(probe)) => probe,
            Some(Err(msg)) => {
                seen_paths.insert(path);
                out.push((lit.to_string(), Err(msg)));
                continue;
            }
            None => continue,
        };
        if unmanglable_names(&probe)
            .iter()
            .any(|n| reserved.blocks(n, &path))
        {
            continue;
        }
        // Reserve this candidate's whole injected face for later
        // candidates: a namespace request floods EVERY top-level decl
        // into the entry (not just exports), so a later candidate's
        // unmanglable name colliding with any of them would redeclare.
        // Export faces ride along for the aliased spellings.
        for s in &probe.stmts {
            let inner = match s {
                Stmt::ExportDecl {
                    inner: Some(inner), ..
                } => inner,
                other => other,
            };
            if let Some(n) = decl_name(inner) {
                reserved.by_path.remove(&n);
                reserved.hard.insert(n);
            }
        }
        for n in collect_own_export_names(&probe.stmts) {
            reserved.by_path.remove(&n);
            reserved.hard.insert(n);
        }
        seen_paths.insert(path.clone());
        out.push((lit.to_string(), Ok(path)));
    }
    out
}

/// Names the walk-time deconflict census cannot rename when this
/// candidate's section injects: class / type decls (knife C/D scope —
/// `top_value_decl_name` answers None) and Fn/Let decls the lib
/// rebinds elsewhere (the census declines those to keep the blind
/// arena rewrite sound). What the set holds is each decl's INJECTED
/// spelling — a bare-exported decl (`export { a as b }`) injects
/// under its face, and the census mangles on that surface too, so
/// only the same two categories stay un-renamable. A collision on
/// any of these redeclares at walk time, so the caller must DROP.
fn unmanglable_names(probe: &Ast) -> HashSet<String> {
    let bare = collect_bare_exports(&probe.stmts);
    let mut out: HashSet<String> = HashSet::new();
    for (i, s) in probe.stmts.iter().enumerate() {
        let inner = match s {
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => inner,
            other => other,
        };
        let Some(n) = decl_name(inner) else { continue };
        // Knife D — ClassDecl mangles now, so a colliding class no
        // longer forces a DROP; the prediction mirrors the census's
        // extra class decline (a type param spelled like the class).
        let manglable = top_value_decl_name(s).is_some()
            && !rebinds_elsewhere(probe, &probe.stmts, 0, i, &n)
            && !(is_class_decl(s) && type_param_shadows(&probe.stmts, &n));
        if !manglable {
            out.insert(bare.get(&n).cloned().unwrap_or(n));
        }
    }
    out
}

/// Top-level names a candidate's injected exports must not shadow.
/// `hard` holds the entry's own decls (export-wrapped included) and
/// namespace aliases; `by_path` maps a static import BINDING to the
/// path it resolves from — a binding is that path's own decl
/// (§16.2.1.6 one-set-of-bindings), so a dyn candidate for the SAME
/// path revisiting the same spelling is not a collision (the ns walk
/// reuses the already-injected decl through the ledger). Same
/// posture as `deconflict::SeedNames`.
struct ReservedNames {
    hard: HashSet<String>,
    by_path: std::collections::HashMap<String, PathBuf>,
}

impl ReservedNames {
    fn blocks(&self, name: &str, candidate_path: &Path) -> bool {
        self.hard.contains(name) || self.by_path.get(name).is_some_and(|p| p != candidate_path)
    }

    fn reserve_binding(&mut self, name: String, path: Option<&Path>) {
        match path {
            Some(p) if !self.hard.contains(&name) => match self.by_path.entry(name.clone()) {
                std::collections::hash_map::Entry::Occupied(e) => {
                    if e.get() != p {
                        e.remove();
                        self.hard.insert(name);
                    }
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(p.to_path_buf());
                }
            },
            _ => {
                self.by_path.remove(&name);
                self.hard.insert(name);
            }
        }
    }
}

fn entry_reserved_names(ast: &Ast, base_dir: &Path) -> ReservedNames {
    let mut names = ReservedNames {
        hard: HashSet::new(),
        by_path: std::collections::HashMap::new(),
    };
    for s in &ast.stmts {
        let inner = match s {
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => inner,
            other => other,
        };
        if let Some(n) = decl_name(inner) {
            names.by_path.remove(&n);
            names.hard.insert(n);
        }
        if let Stmt::ImportDecl {
            source,
            named,
            default,
            namespace,
            ..
        } = s
        {
            let path = resolve_path(base_dir, source).ok();
            for (orig, alias) in named {
                let n = alias.clone().unwrap_or_else(|| orig.clone());
                names.reserve_binding(n, path.as_deref());
            }
            if let Some(d) = default {
                names.reserve_binding(d.clone(), path.as_deref());
            }
            if let Some(ns) = namespace {
                // The namespace ALIAS binding is entry-scope, not any
                // lib's own decl — always hard.
                names.reserve_binding(ns.clone(), None);
            }
        }
    }
    names
}

/// Parse the candidate into a throwaway arena. `None` = unreadable —
/// not a candidate; `Some(Err(reason))` = readable but not module code
/// tr compiles (the same three judgements `load_lib_section` makes of
/// a requested module, minus the link).
fn speculative_probe(path: &Path) -> Option<Result<Ast, String>> {
    let src = std::fs::read_to_string(path).ok()?;
    let tokens = match lexer::tokenize(&src) {
        Ok(t) => t,
        Err(e) => return Some(Err(format!("lex: {e}"))),
    };
    let mut probe = Ast::default();
    if let Err(e) = parser::parse_into(&src, &tokens, &mut probe) {
        return Some(Err(format!("parse: {e}")));
    }
    if let Some(msg) = crate::ast::early_label_errors_in(&probe.stmts, &probe.exprs) {
        return Some(Err(msg));
    }
    if let Some(msg) = crate::ast_early_redecl::module_top_level_redecl(&probe.stmts) {
        return Some(Err(msg));
    }
    Some(Ok(probe))
}
