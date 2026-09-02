//! `desugar_classes` method-owner table + override-chain classification
//! (chunk 180, 2026-06-28).
//!
//! Extracted from `ast/desugar_classes.rs` (Pass 1 sub-section after
//! `full_fields` build, before Phase H.3.b dispatcher emit). Pure read
//! of `class_index` + `parent_map`; returns
//! `(method_owners, chain_methods)` consumed by H.3.b dispatcher
//! synthesis + Pass 2 Call-shape rewriting + the final
//! `ast.method_owners` assignment.
//!
//! Phase I.1 rules enforced here:
//!   * accessor methods (`get X` / `set X`) skipped — they route
//!     through `__cm_<C>__X_get` / `_set` directly, not through
//!     `__dispatch_<M>`.
//!   * a method name is "chain" (gets `__dispatch_<M>`) iff
//!     `owners[0]` (source-first) is an ancestor of every other
//!     owner; otherwise sibling-class case stays Member-shape.
//!   * a method name takes a VTABLE SLOT ([`vtable_methods`]) iff
//!     any owner overrides another — a superset of "chain": a name
//!     that an unrelated class also declares (or that two sibling
//!     subclasses introduce below a silent base) keeps its
//!     Member-shape call sites, and the sibling-dispatch lane routes
//!     those through the slot whenever the receiver's static class
//!     has an overriding descendant (rotation 507 — the old
//!     chain-only slot set left `(b: Base = new Leaf()).name()` on
//!     `Base`'s body as soon as an unrelated `Alone.name()` existed).
//!
//! No `&mut Ast` mutation — same lowest-risk pure-fn extraction
//! pattern as chunks 177/178/179.

use super::desugar_classes_super::ClassIndexEntry;
use super::{mangle_key, method_owner_is_in_chain};
use std::collections::{HashMap, HashSet};

pub(super) fn compute_method_owners_and_chain_methods(
    class_index: &[ClassIndexEntry],
    parent_map: &HashMap<String, Option<String>>,
) -> (HashMap<String, Vec<String>>, HashSet<String>) {
    // method name → ordered list of declaring classes. Source order
    // (deepest sub last) — this matters for dispatcher emission since
    // we walk in reverse to check the deepest class first. Tracks
    // every class that declares a method body, including overrides.
    let mut method_owners: HashMap<String, Vec<String>> = HashMap::new();
    for (_, cname, _tp, _, _, _, _, methods, _) in class_index {
        for m in methods {
            // P8.2 — accessor methods (`get X` / `set X`) don't go through
            // the `__dispatch_<M>` method-dispatch path; `c.X` /
            // `c.X = v` are Member access / Assign sites that ssa_lower
            // routes to the synthesised `__cm_<C>__X_get` / `_set`
            // directly via the accessor side-channel maps. Keeping them
            // out of `method_owners` prevents (a) a spurious
            // `__dispatch_X` body that calls the never-emitted
            // `__cm_<C>__X` (collision with the regular-method name)
            // and (b) a same-name getter+setter pair (or accessor +
            // regular method) from being miscounted as the override-
            // case multi-owner chain.
            if m.accessor_kind.is_some() {
                continue;
            }
            method_owners
                .entry(mangle_key(&m.name).into_owned())
                .or_default()
                .push(cname.clone());
        }
    }
    // Phase I.1 — categorize each multi-owner method. If owners[0]
    // (source-first, the topmost in source order) is an ancestor of
    // every other owner, the method forms a single inheritance chain
    // and gets the `__dispatch_<M>` runtime-tag dispatcher (override
    // case). Otherwise (siblings in unrelated hierarchies, or a mix),
    // call sites stay as Member-shape and ssa_lower picks the right
    // `__cm_<C>__M` from obj's static type.
    let chain_methods: HashSet<String> = method_owners
        .iter()
        .filter(|(_, owners)| owners.len() > 1)
        .filter(|(_, owners)| {
            let base = &owners[0];
            owners
                .iter()
                .skip(1)
                .all(|sub| method_owner_is_in_chain(parent_map, base, sub))
        })
        .map(|(n, _)| n.clone())
        .collect();
    (method_owners, chain_methods)
}

/// Name-sorted method names that need a vtable slot: every name some
/// owner overrides (one owner is a strict descendant of another),
/// whatever else declares it. Sorted so slot numbering is stable.
pub(super) fn vtable_methods(
    method_owners: &HashMap<String, Vec<String>>,
    parent_map: &HashMap<String, Option<String>>,
) -> Vec<String> {
    let mut out: Vec<String> = method_owners
        .iter()
        .filter(|(_, owners)| {
            owners.iter().any(|a| {
                owners
                    .iter()
                    .any(|b| a != b && method_owner_is_in_chain(parent_map, a, b))
            })
        })
        .map(|(n, _)| n.clone())
        .collect();
    out.sort();
    out
}
