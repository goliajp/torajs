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
//! After the BFS drains, [`check`] applies ResolveExport's two
//! failure verdicts to every seeded binding (§16.2.1.6.2 "does not
//! resolve" — circular or missing — and §16.2.1.6.3 "ambiguous"),
//! turning what used to be a silent pick-one / silent skip into the
//! spec-mandated SyntaxError. The dyn-import lane keeps its own
//! per-candidate promise-reject poisoning (`poisoned_candidates`) —
//! a dynamic candidate must never fail the build.

use crate::ast::{Ast, Expr, Stmt};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::resolve_helpers::{NsAccum, queue_nested_import};
use super::{WorkItem, is_builtin_module_source};
use std::collections::VecDeque;

/// One entry-side static request: the user-facing spelling (for the
/// error message) and the binding name its declaration lands under.
pub(super) type StaticRequest = (String, String);

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
                named,
                default,
                namespace,
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
                        let face = alias.as_deref().unwrap_or(orig).to_string();
                        requests.push((face.clone(), face));
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
    })
}

/// Post-BFS judgment on everything the seed recorded: every static
/// request must have resolved ([`check`]), and every entry-side
/// write to an import binding rewrites into its runtime TypeError
/// ([`reject_import_binding_writes`]).
pub(super) fn finalize(
    ast: &mut Ast,
    seed: &EntrySeed,
    injections_by_path: &HashMap<PathBuf, Vec<Stmt>>,
    ns_accums: &HashMap<String, NsAccum>,
    ambiguous_locals: &HashSet<String>,
) -> Result<(), String> {
    check(
        &seed.requests,
        injections_by_path,
        ns_accums,
        ambiguous_locals,
    )?;
    reject_import_binding_writes(ast, &seed.import_bindings, seed.entry_expr_len);
    Ok(())
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

/// §16.2.1.6.2/.3 — the entry's static requests, judged after the BFS
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
    requests: &[StaticRequest],
    injections_by_path: &HashMap<PathBuf, Vec<Stmt>>,
    ns_accums: &HashMap<String, NsAccum>,
    ambiguous_locals: &HashSet<String>,
) -> Result<(), String> {
    if requests.is_empty() {
        return Ok(());
    }
    let mut declared: HashSet<String> = HashSet::new();
    for stmts in injections_by_path.values() {
        super::dyn_import::collect_decl_names(stmts, &mut declared);
    }
    for (face, binding) in requests {
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
    }
    Ok(())
}
