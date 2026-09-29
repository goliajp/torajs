//! Entry-side static resolution — which bindings the ENTRY file's
//! static clauses request, and whether each one actually resolved
//! once the BFS drained. Split from `modules.rs` (file budget) when
//! the §16.2.1.6.2/.3 loud-reject knife landed: the parent keeps BFS
//! orchestration, this file answers "what did the entry ask for, and
//! did it get it".
//!
//! Two clause families seed requests here:
//! - `import { x } from "m"` — the binding is importer-visible under
//!   the requested spelling.
//! - `export { x } from "m"` (named, no star) — §16.2.1.5 must
//!   resolve the indirect export even though nothing imports the
//!   entry, and the source module still evaluates. The binding is
//!   NOT a local one (spec: indirect exports bind nothing in the
//!   module's own scope), so it lands under a synthetic
//!   `__reex_iee<k>_<face>` spelling — the `__reex_` family is
//!   already census-exempt and promote-gate-known — and can never
//!   collide with a user declaration.
//!
//! After the BFS drains, [`finalize`] applies ResolveExport's two
//! failure verdicts ([`check`]) to every seeded binding — the
//! entry's and, since rotation 630, every lib's ImportEntry
//! ([`NestedRequest`], §16.2.1.6.4 step 7 runs per module) — (§16.2.1.6.2 "does not
//! resolve" — circular or missing — and §16.2.1.6.3 "ambiguous"),
//! turning what used to be a silent pick-one / silent skip into the
//! spec-mandated SyntaxError. The dyn-import lane keeps its own
//! per-candidate promise-reject poisoning (`poisoned_candidates`) —
//! a dynamic candidate must never fail the build.

use crate::ast::{Ast, Expr, Stmt};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::graph::ModuleGraph;
use super::resolve_helpers::{NsAccum, queue_nested_import};
use super::{NamedImport, WorkItem, is_builtin_module_source};
use std::collections::VecDeque;

/// One entry-side static request: the user-facing spelling (for the
/// error message) and the binding name its declaration lands under.
pub(super) type StaticRequest = (String, String);

/// One LIB-side ImportEntry (§16.2.1.6.4 step 7 runs for every
/// module in the graph, not just the entry): the module that wrote
/// the clause, the export name it asked for, and the binding its
/// declaration lands under. Recorded by the walk that queued the
/// request, judged by [`finalize`] once the BFS drained — a
/// requester the entry's static clauses cannot reach belongs to a
/// dynamic candidate, whose promise rejects instead of the build.
pub(super) struct NestedRequest {
    pub(super) requester: PathBuf,
    pub(super) face: String,
    pub(super) binding: String,
}

/// The walk's handle on the nested ledger: which module is asking,
/// where the requests go, and the sequence the synthetic
/// `__reex_lie<k>_<face>` bindings draw from (one counter per
/// resolver run — the entry's own indirect exports use `__reex_iee`,
/// a namespace field uses `__reex_<ns>_`; the family is census-exempt
/// and promote-gate-known by prefix).
pub(super) struct RequestLedger<'a> {
    pub(super) requester: &'a Path,
    pub(super) nested: &'a mut Vec<NestedRequest>,
    pub(super) lie_seq: &'a mut usize,
    /// (module, face) pairs that already minted a synthetic binding.
    /// A module walks once per request shape, and a fresh name per
    /// walk would re-queue the same clause forever through a
    /// re-export cycle (A: `export { x } from "./B"`, B the reverse) —
    /// the per-path injected ledger only deduplicates a SPELLING.
    pub(super) lie_seen: &'a mut HashSet<(PathBuf, String)>,
}

/// The resolver-wide half of the nested ledger — every request the
/// walks recorded, the synthetic-binding sequence, and its (module,
/// face) memo. One per `resolve_imports` run; each walk borrows a
/// [`RequestLedger`] view of it for the module it is walking.
#[derive(Default)]
pub(super) struct LedgerState {
    pub(super) nested: Vec<NestedRequest>,
    lie_seq: usize,
    lie_seen: HashSet<(PathBuf, String)>,
}

impl LedgerState {
    pub(super) fn ledger<'a>(&'a mut self, requester: &'a Path) -> RequestLedger<'a> {
        RequestLedger {
            requester,
            nested: &mut self.nested,
            lie_seq: &mut self.lie_seq,
            lie_seen: &mut self.lie_seen,
        }
    }
}

impl RequestLedger<'_> {
    /// One request: the export name asked for (for the message) and
    /// the binding its declaration lands under.
    pub(super) fn record(&mut self, face: &str, binding: &str) {
        self.nested.push(NestedRequest {
            requester: self.requester.to_path_buf(),
            face: face.to_string(),
            binding: binding.to_string(),
        });
    }

    /// A lib's `import` clause (§16.2.1.6.4 step 7). A requested
    /// spelling always lands importer-visible under `alias ?? orig`
    /// (`inject_export_inner` injects per visible spelling; the
    /// 427-02 class-alias census rename lands on the alias too), and
    /// the default lane materializes `let <alias>` (a repeat request
    /// binds `let <second> = <first>`), so the binding IS the visible
    /// name. Only ImportDecls record — a request `queue_star_reexport`
    /// forwards lands from whichever star provides it, so recording
    /// the forwarded copy would misreport a name a later star
    /// supplies.
    pub(super) fn import_clause(
        &mut self,
        source: &str,
        named: &[NamedImport],
        default: &Option<String>,
    ) {
        if is_builtin_module_source(source) {
            return;
        }
        for (orig, alias) in named {
            let binding = alias.as_deref().unwrap_or(orig).to_string();
            self.record(orig, &binding);
        }
        if let Some(d) = default {
            self.record("default", d);
        }
    }

    /// A fresh binding for an indirect export nobody requested
    /// (§16.2.1.6.4 step 9 still has to resolve it) — `None` when
    /// this module's clause already minted one.
    pub(super) fn synth_binding(&mut self, face: &str) -> Option<String> {
        if !self
            .lie_seen
            .insert((self.requester.to_path_buf(), face.to_string()))
        {
            return None;
        }
        *self.lie_seq += 1;
        Some(format!("__reex_lie{}_{face}", *self.lie_seq))
    }
}

/// The modules the ENTRY file requests, in clause order, onto the
/// worklist; answers the static-request ledger [`check`] verifies
/// after the BFS drains. Entry `import`s seed named/default/namespace
/// requests; a named `export { x } from "m"` seeds a synthetic-alias
/// named request (or a default-lane request for `export { default }
/// from`); `export * from "m"` binds nothing anyone can see but
/// §16.2.1.5 still evaluates the module it names.
pub(super) struct EntrySeed {
    pub(super) requests: Vec<StaticRequest>,
    import_bindings: HashSet<String>,
    /// The arena length BEFORE any lib parses — marks which
    /// expressions the entry itself wrote. The import-binding write
    /// rejection scopes to exactly those, and the dead-copy sweep
    /// (`dead_lib_exprs`) reads it as the tombstone floor: entry
    /// arena is never a candidate.
    pub(super) entry_expr_len: usize,
    /// The modules the entry's STATIC clauses name — the roots the
    /// nested ledger's build-failing half is reachable from. Filled
    /// by `seed_requests` once the static seed has queued.
    pub(super) static_roots: Vec<PathBuf>,
    /// (candidate path, `__dyn_ns_c<k>`) per §13.3.10 candidate — a
    /// nested request only a candidate reaches poisons that
    /// candidate's namespace instead of failing the build.
    pub(super) dyn_roots: Vec<(PathBuf, String)>,
    /// The entry file's canonical path when it has one (stdin has
    /// none) — a side-effect request naming it evaluates nothing
    /// extra, the entry IS the program (`repeat_request`).
    pub(super) entry_path: Option<PathBuf>,
}

pub(super) fn seed_entry_requests(
    ast: &Ast,
    base_dir: &Path,
    work: &mut VecDeque<WorkItem>,
) -> Result<EntrySeed, String> {
    let entry_expr_len = ast.exprs.len();
    let mut requests: Vec<StaticRequest> = Vec::new();
    let mut import_bindings: HashSet<String> = HashSet::new();
    let mut iee_seq = 0usize;
    for s in &ast.stmts {
        match s {
            Stmt::ImportDecl {
                source,
                type_only: true,
                ..
            } => super::types_lane::queue_types(work, base_dir, source)?,
            // erased whole: `export type { … } from` requests nothing
            Stmt::ExportDecl {
                type_only: true, ..
            } => {}
            Stmt::ImportDecl {
                source,
                named,
                default,
                namespace,
                ..
            } => {
                // Every declared import binding is immutable in the
                // importer (§16.2.1.5 CreateImportBinding) — builtin
                // modules included; see [`reject_import_binding_writes`].
                for (orig, alias) in named {
                    import_bindings.insert(alias.as_deref().unwrap_or(orig).to_string());
                }
                if let Some(d) = default {
                    import_bindings.insert(d.clone());
                }
                if let Some(ns) = namespace {
                    import_bindings.insert(ns.clone());
                }
                // Builtin modules (fs, path, ...) inject no decls —
                // their bindings resolve inside the checker — so they
                // must not enter the resolution ledger.
                if !is_builtin_module_source(source) {
                    for (orig, alias) in named {
                        let binding = alias.as_deref().unwrap_or(orig).to_string();
                        requests.push((orig.clone(), binding));
                    }
                    // `import x from` / `import { default as x }` —
                    // ResolveExport("default") is null when the module
                    // never wrote one, and §16.2.1.6.3 step 6 says a
                    // star cannot supply it (the star pour never
                    // forwards the default lane, so nothing lands).
                    if let Some(d) = default {
                        requests.push(("default".to_string(), d.clone()));
                    }
                }
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
            Stmt::ExportDecl {
                star: None,
                source: Some(src),
                named,
                ..
            } if named.is_empty() && !is_builtin_module_source(src) => {
                // `export {} from "m"` binds nothing, but §16.2.2.5
                // ModuleRequests still lists m — it loads (and its
                // parse errors are this program's) and evaluates.
                queue_nested_import(work, base_dir, src, Vec::new(), None, None)?;
            }
            Stmt::ExportDecl {
                star: None,
                source: Some(src),
                named,
                ..
            } if !named.is_empty() && !is_builtin_module_source(src) => {
                let mut nested_named: Vec<(String, Option<String>)> = Vec::new();
                let mut default_binding: Option<String> = None;
                for (orig, alias) in named {
                    let face = alias.as_deref().unwrap_or(orig);
                    let binding = format!("__reex_iee{iee_seq}_{face}");
                    iee_seq += 1;
                    if orig == "default" {
                        // The source's default only answers on the
                        // default lane (`export { default as x } from`).
                        default_binding = Some(binding.clone());
                    } else {
                        nested_named.push((orig.clone(), Some(binding.clone())));
                    }
                    requests.push((face.to_string(), binding));
                }
                queue_nested_import(work, base_dir, src, nested_named, default_binding, None)?;
            }
            _ => {}
        }
    }
    Ok(EntrySeed {
        requests,
        import_bindings,
        entry_expr_len,
        static_roots: Vec::new(),
        dyn_roots: Vec::new(),
        entry_path: None,
    })
}

/// Post-BFS judgment on everything the seed and the walks recorded:
/// every static request must have resolved ([`check`]) — the
/// entry's own, and every lib request the entry's static clauses can
/// reach; a lib request only a dynamic candidate reaches poisons
/// that candidate (returned, for the dispatcher's SyntaxError
/// reject) — and every entry-side write to an import binding
/// rewrites into its runtime TypeError
/// ([`reject_import_binding_writes`]).
pub(super) fn finalize(
    ast: &mut Ast,
    seed: &EntrySeed,
    graph: &ModuleGraph,
    nested: &[NestedRequest],
    injections_by_path: &HashMap<PathBuf, Vec<Stmt>>,
    ns_accums: &HashMap<String, NsAccum>,
    ambiguous_locals: &HashSet<String>,
    entry_self_bindings: &HashSet<String>,
) -> Result<HashSet<String>, String> {
    let mut declared: HashSet<String> = HashSet::new();
    for stmts in injections_by_path.values() {
        super::dyn_import_poison::collect_decl_names(stmts, &mut declared);
    }
    declared.extend(entry_self_bindings.iter().cloned());
    let verdict =
        |face: &str, binding: &str| check(face, binding, &declared, ns_accums, ambiguous_locals);
    for (face, binding) in &seed.requests {
        verdict(face, binding)?;
    }
    let static_reach = graph.reachable(&seed.static_roots);
    let mut poisoned: HashSet<String> = HashSet::new();
    for r in nested {
        let Err(e) = verdict(&r.face, &r.binding) else {
            continue;
        };
        if static_reach.contains(&r.requester) {
            return Err(e);
        }
        for (root, ns) in &seed.dyn_roots {
            if graph
                .reachable(std::slice::from_ref(root))
                .contains(&r.requester)
            {
                poisoned.insert(ns.clone());
            }
        }
    }
    reject_import_binding_writes(ast, &seed.import_bindings, seed.entry_expr_len);
    Ok(poisoned)
}

/// §16.2.1.5 CreateImportBinding — assignment to an import binding is
/// a RUNTIME TypeError, not a static reject: the checker's type
/// mismatch rejected whole programs whose assignment sits inside a
/// `try` (test262 probes binding immutability exactly that way, e.g.
/// the instn-iee-bndng family's `B = null`). Rewrite every
/// entry-arena `Assign` / `PostIncr` node targeting an import
/// binding into a TypeError-throw IIFE (the statement-in-value-
/// position carrier — the arrow is constructed here at module scope,
/// so the closure lift sees a live construction site).
///
/// Only expressions the ENTRY itself wrote rewrite (`i <
/// entry_expr_len`, the arena length before any lib parsed): a lib
/// module assigning its OWN exported binding is the legal
/// live-binding write path, and every lib expr sits past the mark.
fn reject_import_binding_writes(ast: &mut Ast, bindings: &HashSet<String>, entry_expr_len: usize) {
    if bindings.is_empty() {
        return;
    }
    for i in 0..entry_expr_len {
        let target = match &ast.exprs[i] {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => *target,
            _ => continue,
        };
        let Expr::Ident(name) = ast.get_expr(target) else {
            continue;
        };
        if !bindings.contains(name) {
            continue;
        }
        let name = name.clone();
        let msg = ast.add_expr(Expr::String(
            format!("Assignment to import binding '{name}'.").into(),
        ));
        let exc = ast.add_expr(Expr::New {
            class_name: "TypeError".to_string(),
            args: vec![msg],
            type_args: Vec::new(),
        });
        let arrow = ast.add_expr(Expr::ArrowFn {
            params: Vec::new(),
            return_type: None,
            body: vec![Stmt::Throw(exc)],
        });
        ast.exprs[i] = Expr::Call {
            callee: arrow,
            args: Vec::new(),
        };
    }
}

/// §16.2.1.6.2/.3 — one static request, judged after the BFS
/// drained. A binding resolves when a declaration landed under it
/// (any decl kind — the default lane materializes a `let`, type
/// requests a `TypeDecl`) or a namespace accumulator claimed it
/// (`export * as ns from` faces materialize post-BFS). "Ambiguous"
/// (two `export * from` clauses landing the requested spelling from
/// different modules) and "does not resolve" (circular or missing
/// indirect-export chains deduplicate to nothing in the BFS) both
/// take the spec's SyntaxError — the message carries the marker the
/// CLI's `import error:` prefix composes into a recognizable
/// resolution reject.
fn check(
    face: &str,
    binding: &str,
    declared: &HashSet<String>,
    ns_accums: &HashMap<String, NsAccum>,
    ambiguous_locals: &HashSet<String>,
) -> Result<(), String> {
    if ambiguous_locals.contains(binding) {
        return Err(format!(
            "module resolution failure (SyntaxError): requested binding '{face}' is ambiguous — multiple star exports provide it"
        ));
    }
    if !declared.contains(binding) && !ns_accums.contains_key(binding) {
        return Err(format!(
            "module resolution failure (SyntaxError): requested binding '{face}' does not resolve (missing or circular export)"
        ));
    }
    Ok(())
}
