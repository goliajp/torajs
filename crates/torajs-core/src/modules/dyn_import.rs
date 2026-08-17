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
//!    form uses (`__dyn_ns_c<k>` aliases ride the existing
//!    `inline_dyn_ns_objlits` rewrite).
//! 2. [`synth_dispatcher`] — the runtime half, per §13.3.10 evaluation:
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

use crate::ast::{Ast, BinOp, Expr, Param, Stmt};
use crate::lexer;
use crate::parser;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use super::deconflict::{
    is_class_decl, rebinds_elsewhere, top_value_decl_name, type_param_shadows,
};
use super::resolve_helpers::{collect_bare_exports, collect_own_export_names};
use super::{WorkItem, decl_name, resolve_path};

/// The dispatcher FnDecl's name. Parser-minted at every `import()`
/// call site; no user identifier can collide (double-underscore
/// reserve).
const DISPATCHER_NAME: &str = "__torajs_dyn_import";

/// Queue every candidate module onto the BFS worklist under a
/// `__dyn_ns_c<k>` namespace alias (riding the existing
/// `inline_dyn_ns_objlits` rewrite) and answer the
/// `(specifier literal, alias)` table the dispatcher matches against.
/// No-op (empty table) when the entry has no dynamic import.
pub(super) fn seed_candidates(
    ast: &Ast,
    base_dir: &Path,
    work: &mut VecDeque<WorkItem>,
) -> Vec<(String, String)> {
    let mut table: Vec<(String, String)> = Vec::new();
    if ast.dyn_import_present {
        for (k, (lit, path)) in collect_candidates(ast, base_dir).into_iter().enumerate() {
            let ns_name = format!("__dyn_ns_c{k}");
            work.push_back((path, Vec::new(), None, false, Some(ns_name.clone()), false));
            table.push((lit, ns_name));
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
///   candidate must not turn the build into a parse error);
/// - its UNMANGLABLE names don't collide with the entry's reserved
///   names or an earlier candidate's injected face (see module doc);
///   every other collision deconflicts at walk time.
fn collect_candidates(ast: &Ast, base_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut reserved = entry_reserved_names(ast, base_dir);
    let mut seen_lits: HashSet<&str> = HashSet::new();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for e in &ast.exprs {
        let Expr::String(lit) = e else { continue };
        if !(lit.starts_with("./") || lit.starts_with("../")) {
            continue;
        }
        if !seen_lits.insert(lit.as_str()) {
            continue;
        }
        let Ok(path) = resolve_path(base_dir, lit) else {
            continue;
        };
        if seen_paths.contains(&path) {
            // Two spellings of one file: the first literal claimed it.
            continue;
        }
        let Some(probe) = speculative_probe(&path) else {
            continue;
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
        out.push((lit.clone(), path));
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

/// Parse the candidate into a throwaway arena. `None` = unreadable or
/// unparseable — not a candidate.
fn speculative_probe(path: &Path) -> Option<Ast> {
    let src = std::fs::read_to_string(path).ok()?;
    let tokens = lexer::tokenize(&src).ok()?;
    let mut probe = Ast::default();
    parser::parse_into(&src, &tokens, &mut probe).ok()?;
    Some(probe)
}

/// Synthesize the dispatcher FnDecl:
///
/// ```ts
/// function __torajs_dyn_import(__spec: any): any {
///   try {
///     const __s: any = String(__spec);
///     if (__s === "<lit_0>") return Promise.resolve(__dyn_ns_c0);
///     ...
///     throw new TypeError("Failed to resolve module specifier '" + __s + "'");
///   } catch (__e) {
///     return Promise.reject(__e);
///   }
/// }
/// ```
///
/// The whole §13.3.10 abrupt surface funnels through ONE catch: a
/// throwing `toString` (step 7) and the miss TypeError both land in
/// `Promise.reject`, which is exactly IfAbruptRejectPromise. The
/// `__dyn_ns_c<k>` idents rewrite to namespace object literals in
/// `inline_dyn_ns_objlits` — synthesis must run before that pass.
pub(super) fn synth_dispatcher(ast: &mut Ast, table: &[(String, String)]) -> Stmt {
    let mut body: Vec<Stmt> = Vec::new();
    // const __s: any = String(__spec);
    let string_ident = ast.add_expr(Expr::Ident("String".to_string()));
    // §13.3.10 runs the intrinsic ToString — like the Promise idents
    // below, no `with` object may substitute these globals.
    ast.synth_global_refs.insert(string_ident);
    let spec_ident = ast.add_expr(Expr::Ident("__spec".to_string()));
    let s_init = ast.add_expr(Expr::Call {
        callee: string_ident,
        args: vec![spec_ident],
    });
    body.push(Stmt::LetDecl {
        mutable: false,
        name: "__s".to_string(),
        type_ann: Some("any".to_string()),
        init: s_init,
        is_var: false,
    });
    // Per candidate: if (__s === "<lit>") return Promise.resolve(<ns>);
    for (lit, ns_name) in table {
        let s_ref = ast.add_expr(Expr::Ident("__s".to_string()));
        let lit_expr = ast.add_expr(Expr::String(lit.clone()));
        let cond = ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left: s_ref,
            right: lit_expr,
        });
        let ns_ident = ast.add_expr(Expr::Ident(ns_name.clone()));
        let promise_ident = ast.add_expr(Expr::Ident("Promise".to_string()));
        ast.synth_global_refs.insert(promise_ident);
        let resolve = ast.add_expr(Expr::Member {
            obj: promise_ident,
            name: "resolve".to_string(),
        });
        let resolved = ast.add_expr(Expr::Call {
            callee: resolve,
            args: vec![ns_ident],
        });
        body.push(Stmt::If {
            cond,
            then_branch: Box::new(Stmt::Return(Some(resolved))),
            else_branch: None,
        });
    }
    // throw new TypeError("Failed to resolve module specifier '" + __s + "'");
    let msg_head = ast.add_expr(Expr::String(
        "Failed to resolve module specifier '".to_string(),
    ));
    let s_ref = ast.add_expr(Expr::Ident("__s".to_string()));
    let head_concat = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: msg_head,
        right: s_ref,
    });
    let msg_tail = ast.add_expr(Expr::String("'".to_string()));
    let msg = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: head_concat,
        right: msg_tail,
    });
    let type_error = ast.add_expr(Expr::New {
        class_name: "TypeError".to_string(),
        args: vec![msg],
        type_args: Vec::new(),
    });
    body.push(Stmt::Throw(type_error));
    // catch (__e) { return Promise.reject(__e); }
    let promise_ident = ast.add_expr(Expr::Ident("Promise".to_string()));
    ast.synth_global_refs.insert(promise_ident);
    let reject = ast.add_expr(Expr::Member {
        obj: promise_ident,
        name: "reject".to_string(),
    });
    let e_ident = ast.add_expr(Expr::Ident("__e".to_string()));
    let rejected = ast.add_expr(Expr::Call {
        callee: reject,
        args: vec![e_ident],
    });
    let catch_body = vec![Stmt::Return(Some(rejected))];
    Stmt::FnDecl {
        name: DISPATCHER_NAME.to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            name: "__spec".to_string(),
            type_ann: Some("any".to_string()),
            default: None,
            is_rest: false,
        }],
        return_type: Some("any".to_string()),
        body: vec![Stmt::Try {
            body,
            had_catch: true,
            catch_param: Some("__e".to_string()),
            catch_type: None,
            catch_body,
            finally_body: None,
        }],
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    }
}
