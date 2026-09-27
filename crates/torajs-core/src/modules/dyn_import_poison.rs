//! The link's poison verdict per dynamic-import candidate: which
//! candidates the dispatcher must answer with a SyntaxError reject
//! instead of their namespace. Split from `dyn_import_dispatcher.rs`
//! to keep that file under the size line.

use crate::ast::Stmt;
use std::collections::{HashMap, HashSet};

use super::decl_name;
use super::dyn_import::DynTarget;
use super::resolve_helpers::NsAccum;

/// 426-01 — candidates whose namespace depends on a re-export binding
/// that never materialized. A circular indirect-export chain
/// (`export { x } from` A→B→A) deduplicates to nothing in the BFS:
/// the synthetic `__reex_<ns>_<face>` binding is requested but no
/// declaration ever injects. Per §16.2.1.5 the module fails to
/// instantiate with a SyntaxError — and a dyn-import candidate must
/// reject its promise, never fail the build. A missing (non-circular)
/// indirect export takes the same verdict: same ResolveExport step,
/// same SyntaxError. The dispatcher emits a SyntaxError throw for
/// these entries (riding its own reject-on-abrupt catch) and never
/// references the namespace, so the dangling binding keeps no use
/// site and the checker stays quiet.
/// A candidate is also poisoned when any of its namespace bindings is
/// AMBIGUOUS — two `export * from` clauses landed the same requested
/// spelling from different modules (§16.2.1.6.3 "ambiguous", the
/// t262 instn-iee-err-ambiguous-import family): same ResolveExport
/// verdict, same SyntaxError reject.
pub(super) fn poisoned_candidates(
    table: &[(String, DynTarget)],
    ns_accums: &HashMap<String, NsAccum>,
    injections: &[Stmt],
    ambiguous_locals: &HashSet<String>,
) -> HashSet<String> {
    let namespaces = table.iter().filter_map(|(_, t)| match t {
        DynTarget::Namespace(ns) => Some(ns),
        DynTarget::SyntaxError(_) => None,
    });
    let needs_scan = namespaces
        .clone()
        .any(|ns| ns_accums.get(ns).is_some_and(reex_backed));
    let mut declared: HashSet<String> = HashSet::new();
    if needs_scan {
        collect_decl_names(injections, &mut declared);
    }
    namespaces
        .filter(|ns| {
            ns_accums.get(ns.as_str()).is_some_and(|acc| {
                acc.fields().iter().any(|(_, local)| {
                    ambiguous_locals.contains(local)
                        || (local.starts_with("__reex_") && !declared.contains(local))
                })
            })
        })
        .cloned()
        .collect()
}

fn reex_backed(acc: &NsAccum) -> bool {
    acc.fields()
        .iter()
        .any(|(_, local)| local.starts_with("__reex_"))
}

pub(super) fn collect_decl_names(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::Multi(inner) => collect_decl_names(inner, out),
            _ => {
                if let Some(n) = decl_name(s) {
                    out.insert(n);
                }
            }
        }
    }
}
