//! W-ESC — typed-container any-escape faces (RFC
//! 20260706-typed-arr-any-escape, B1).
//!
//! A typed `Array<T>` that flows into an `any`-annotated slot
//! (`const u: any = t` / any param / any return / `any[]` element)
//! crosses into the runtime `any` world, where JS semantics allow
//! element-kind transitions (`u[0] = "s"` on a `number[]` alias) that
//! the typed tier's raw-slot loads cannot observe. The alloc site
//! itself must therefore re-intern with an Any element (NaN-box
//! slots) — decided at module level over the same container alias
//! classes W4 uses, so caller / callee / global faces never drift
//! (K.3 lesson).
//!
//! Seeds are the any-annotated slot keys themselves — the annotation
//! IS the escape sink. A container reaching such a slot shares its
//! alias class through the existing guarded-alias / container-channel
//! unions; a class no container ever reaches is simply never asked
//! about at the widen site. Conservative direction: an escaped class
//! demotes every array in it — repr cost only, never correctness
//! (same bias as the `by_name` broadcast).

use std::collections::{HashSet, VecDeque};

use super::alias::named_ref;
use super::container::{UnionFind, canon_key_frozen};
use super::{Analysis, SlotKey};

impl<'a> Analysis<'a> {
    /// Record `key` as an any-escape sink when its annotation face
    /// spells a `Type::Any` source (`any` / `unknown` / `object` —
    /// the same set `parse_type` folds to Any). Depth-wrapped so
    /// `box: any[]` seeds the element point (values stored INTO the
    /// box escape; the box itself is already Arr<Any> by annotation).
    pub(super) fn seed_any_face(&mut self, key: &SlotKey, ann: &str) {
        let Some((base, depth)) = named_ref(ann) else {
            return;
        };
        if !matches!(base, "any" | "unknown" | "object") {
            return;
        }
        let mut k = key.clone();
        for _ in 0..depth {
            k = SlotKey::Elem(Box::new(k));
        }
        self.any_seeds.push(k);
    }
}

/// Post-canonicalize propagation: map every seed onto its frozen
/// class representative, then push the escape down the Elem chain —
/// a demoted outer array's inner arrays must demote too (the any
/// side reaches them through the outer's NaN-box slots). Depth-
/// capped: real TS nesting is shallow and an unrepresented Elem key
/// canonicalizes to itself, so the chain would otherwise walk
/// forever.
pub(super) fn propagate(seeds: &[SlotKey], uf: &UnionFind) -> HashSet<SlotKey> {
    const ELEM_DEPTH_CAP: usize = 8;
    let mut out: HashSet<SlotKey> = HashSet::new();
    let mut work: VecDeque<(SlotKey, usize)> = seeds
        .iter()
        .map(|k| (canon_key_frozen(uf, k), 0usize))
        .collect();
    while let Some((k, d)) = work.pop_front() {
        if out.insert(k.clone()) && d < ELEM_DEPTH_CAP {
            let ek = canon_key_frozen(uf, &SlotKey::Elem(Box::new(k)));
            if !out.contains(&ek) {
                work.push_back((ek, d + 1));
            }
        }
    }
    out
}
