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

use crate::ast::{Ast, Stmt};
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
pub(super) fn seed_entry_requests(
    ast: &Ast,
    base_dir: &Path,
    work: &mut VecDeque<WorkItem>,
) -> Result<Vec<StaticRequest>, String> {
    let mut requests: Vec<StaticRequest> = Vec::new();
    let mut iee_seq = 0usize;
    for s in &ast.stmts {
        match s {
            Stmt::ImportDecl {
                source,
                named,
                default,
                namespace,
            } => {
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
    Ok(requests)
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
pub(super) fn check(
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
