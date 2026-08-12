//! `desugar_classes` field-inheritance flattening (chunk 179, 2026-06-28).
//!
//! Extracted from `ast/desugar_classes.rs` (Pass 1 sub-section after
//! parent_map build, before method_owners walk). Pure read of
//! `class_index`; returns the flattened `full_fields` HashMap consumed
//! later in default-init synthesis + Pass 3 TypeDecl emission +
//! factory body assembly. Panics on inheritance-graph violations
//! (forward-reference parent / subclass field collision).
//!
//! M5.2 rules enforced here:
//!   * parent must be declared before subclass (forward refs rejected
//!     so field flattening is trivially order-correct). Rotation 373
//!     (L3b 373-05) narrows this to STATEMENT-position classes: an
//!     expression-position class inside an enclosing class body
//!     (`ast.class_expr_deferred`) evaluates when that body RUNS —
//!     after every top-level definition completed — so its parent may
//!     sit anywhere in the index; the flattening fixpoint below is
//!     its order authority (a truly missing parent still panics).
//!   * subclass cannot redeclare a parent field (TS spec allows same-
//!     type shadow but M5.2.a keeps this simple).
//!
//! Both validations panic on violation (preserving pre-extract
//! behavior). Body verbatim — pure data computation, no `&mut Ast`
//! mutation (lowest-risk extraction pattern per chunks 177/178).

use super::desugar_classes_super::ClassIndexEntry;
use crate::ast::Ast;
use std::collections::{HashMap, HashSet};

pub(super) fn compute_full_fields(
    ast: &Ast,
    class_index: &[ClassIndexEntry],
) -> HashMap<String, Vec<(String, String)>> {
    // Detect missing-parent and cycle errors for statement-position
    // classes — no forward references to classes later in source
    // order (see module doc for the deferred exemption).
    let mut declared_so_far: HashSet<String> = HashSet::new();
    for (_, cname, _tp, parent, _, _, _, _, _) in class_index {
        if let Some(p) = parent
            && !ast.class_expr_deferred.contains(cname)
            && !declared_so_far.contains(p)
        {
            panic!(
                "M5.2: `{cname} extends {p}` — parent class `{p}` must be declared \
                 before `{cname}` (and must exist as a class, not a type alias)"
            );
        }
        declared_so_far.insert(cname.clone());
    }

    // Compute the flattened (full) field list for each class along the
    // inheritance chain: parent's fields followed by self's. This is the
    // layout that `type C = { ... }` will declare and the factory will
    // default-initialize. A worklist fixpoint: an entry resolves once
    // its parent's flattened list exists — source order is one valid
    // schedule, so statement-position classes resolve in the first
    // sweep exactly as the old single pass did, and a deferred entry
    // whose parent comes later resolves in a subsequent sweep. No
    // progress with entries left = an unresolvable (or cyclic) parent.
    let mut full_fields: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut pending: Vec<&ClassIndexEntry> = class_index.iter().collect();
    while !pending.is_empty() {
        let before = pending.len();
        let mut next: Vec<&ClassIndexEntry> = Vec::new();
        for entry in pending {
            let (_, cname, _tp, parent, fields, _, _, _, _) = entry;
            if let Some(p) = parent
                && !full_fields.contains_key(p)
            {
                next.push(entry);
                continue;
            }
            let mut combined: Vec<(String, String)> = Vec::new();
            if let Some(p) = parent {
                // Present by the resolution gate just above.
                let pfields = full_fields.get(p).unwrap_or_else(|| {
                    panic!("internal: parent `{p}` of `{cname}` had no flattened fields")
                });
                combined.extend(pfields.iter().cloned());
            }
            // Everything in `combined` up to here came from the parent, so
            // this index separates "collides with an inherited field" from
            // "declared twice in this class" below.
            let inherited = combined.len();
            for (fn_, ft) in fields {
                // Subclass fields must not collide with parent fields. (TS
                // allows shadowing with the same type, but M5.2.a keeps this
                // simple — disallow.)
                if let Some(at) = combined.iter().position(|(n, _)| n == fn_) {
                    if at < inherited {
                        panic!(
                            "M5.2: subclass `{cname}` redeclares parent field `{fn_}` — \
                             not yet supported"
                        );
                    }
                    // Not an inheritance problem at all: the class declares
                    // `fn_` twice. Reachable from source a user did write —
                    // `*g(x = 0, x)` gives the generator's state-machine
                    // class two fields named `x` — so saying "parent field"
                    // here sent the reader looking up a hierarchy that has
                    // nothing to do with it, naming a class they never wrote.
                    panic!(
                        "M5.2: class `{cname}` declares field `{fn_}` twice — not yet supported"
                    );
                }
                combined.push((fn_.clone(), ft.clone()));
            }
            full_fields.insert(cname.clone(), combined);
        }
        if next.len() == before {
            let (_, cname, _tp, parent, _, _, _, _, _) = next[0];
            let p = parent.as_deref().unwrap_or("<none>");
            panic!(
                "M5.2: `{cname} extends {p}` — parent class `{p}` must be declared \
                 before `{cname}` (and must exist as a class, not a type alias)"
            );
        }
        pending = next;
    }
    full_fields
}
