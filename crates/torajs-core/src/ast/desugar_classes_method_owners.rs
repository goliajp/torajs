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
//!     through `__cm_<C>__X_get` / `_set` directly, never through
//!     `__dispatch_<M>`. After the dispatchers and the call rewrite,
//!     [`finalize_side_tables`] enters each face under `X_get` /
//!     `X_set` so an overridden face takes a vtable slot.
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
//! Only [`finalize_side_tables`] writes the `Ast`, as the desugar's
//! last step.

use super::desugar_classes_super::ClassIndexEntry;
use super::{Ast, PropKey, Stmt, mangle_key, method_owner_is_in_chain};
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
    for (_, cname, _tp, _, _, _, _, methods, _, _) in class_index {
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

/// Tail side-table population: multi-owner `method_owners` for the
/// `__dispatch_<M>` runtime-tag dispatch (single-owner entries are
/// statically rewritten already), name-sorted stable vtable slots in
/// `method_index` (every overridden name, chain or not — see
/// [`vtable_methods`]), the accessor slots, and the accessor-record
/// drain (done last so the maps are complete before any check / lower
/// pass runs; duplicate (class, prop) entries are overwritten — the
/// FnDecl-level redeclaration error halts the pipeline before they're
/// consulted).
pub(super) fn finalize_side_tables(
    ast: &mut Ast,
    mut method_owners: HashMap<String, Vec<String>>,
    accessor_getter_records: Vec<(String, PropKey, String)>,
    accessor_setter_records: Vec<(String, PropKey, String)>,
) {
    let faces = accessor_getter_records
        .iter()
        .chain(&accessor_setter_records);
    ast.accessor_slots = add_accessor_owners(ast, &mut method_owners, faces);
    let slots = vtable_methods(&method_owners, &ast.class_parents);
    ast.method_index = slots
        .into_iter()
        .enumerate()
        .map(|(i, n)| (n, i as u32))
        .collect();
    ast.method_owners = method_owners
        .into_iter()
        .filter(|(_, owners)| owners.len() > 1)
        .collect();
    for (cname, prop, fn_name) in accessor_getter_records {
        ast.accessor_getters.insert((cname, prop), fn_name);
    }
    for (cname, prop, fn_name) in accessor_setter_records {
        ast.accessor_setters.insert((cname, prop), fn_name);
    }
}

/// Enter each instance accessor face under its symbol tail
/// (`<key>_get` / `<key>_set`, the part of `__cm_<C>__<key>_get` after
/// the class), so an overridden face takes a vtable slot like any
/// overridden method; answers the tails entered. This runs after the
/// dispatcher stubs and the call rewrite have read the table: nothing
/// calls a face by name, so neither may see it.
///
/// A whole tail stays out when a regular method also spells it (the
/// slot would mix the two) or when [`barred_tails`] bars it — a face
/// reading `arguments` among them: the arguments-object faces size a
/// method's argv from its direct calls and refuse every slot row. A
/// barred face keeps its direct call, and a receiver with a
/// redeclaring descendant takes the any-member lane, whose boxed call
/// carries the real argv. Per tail, not per face: a slot missing one
/// redeclaring class would hand that class's instances its ancestor's
/// row.
fn add_accessor_owners<'a>(
    ast: &Ast,
    method_owners: &mut HashMap<String, Vec<String>>,
    records: impl Iterator<Item = &'a (String, PropKey, String)>,
) -> HashSet<String> {
    let faces: Vec<(&str, &str, &str)> = records
        .filter_map(|(c, _, f)| {
            let tail = f.strip_prefix(&format!("__cm_{c}__"))?;
            Some((c.as_str(), tail, f.as_str()))
        })
        .collect();
    let mut kept: HashSet<String> = HashSet::new();
    let mut barred: HashSet<&str> = barred_tails(ast, &faces);
    barred.extend(
        faces
            .iter()
            .map(|f| f.1)
            .filter(|t| method_owners.contains_key(*t)),
    );
    for (cname, tail, _) in faces {
        if barred.contains(tail) {
            continue;
        }
        let owners = method_owners.entry(tail.to_string()).or_default();
        owners.push(cname.to_string());
        kept.insert(tail.to_string());
    }
    kept
}

/// The tails no slot may carry, read off the faces' declarations: a
/// face whose body reads `arguments`, and a setter tail whose faces
/// spell their parameters differently. The parameter join would widen
/// such a slot to `any`, and the checker types a write by the
/// receiver's setter declaration — widened, `b.v = "x"` against
/// `set v(x: number)` would pass where the program must be refused.
fn barred_tails<'a>(ast: &Ast, faces: &[(&str, &'a str, &str)]) -> HashSet<&'a str> {
    use super::arguments_object_walkers::body_has_arguments_length as len;
    use super::arguments_object_walkers::body_has_non_length_arguments_touch as touch;
    let by_fn: HashMap<&str, &'a str> = faces.iter().map(|(_, t, f)| (*f, *t)).collect();
    let mut params: HashMap<&str, Vec<Option<&String>>> = HashMap::new();
    let mut out = HashSet::new();
    for s in super::toplevel_stmts_flat(ast) {
        let Stmt::FnDecl {
            name,
            body,
            params: ps,
            ..
        } = s
        else {
            continue;
        };
        let Some(tail) = by_fn.get(name.as_str()) else {
            continue;
        };
        let anns: Vec<Option<&String>> = ps.iter().skip(1).map(|p| p.type_ann.as_ref()).collect();
        if len(ast, body) || touch(ast, body) || params.get(tail).is_some_and(|seen| *seen != anns)
        {
            out.insert(*tail);
        }
        params.entry(tail).or_insert(anns);
    }
    out
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
