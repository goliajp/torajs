//! Which promise value points a promise the walk does not follow can
//! reach (`promise_entry` notes the sources and the number faces):
//! the closure of the sources over the alias classes and pairs, and
//! [`PromiseTaint::widens`], the one predicate the analysis seeds by
//! and lowering's `await` reads ask.

use super::container::{UnionFind, canon_key_frozen};
use super::{Analysis, SlotKey};
use std::collections::{HashMap, HashSet};

/// Is the holder `h` of a promise value point in an untracked class:
/// reached itself, or the element or program-named field of one that
/// is?
fn holder_untracked(tainted: &HashSet<SlotKey>, uf: &UnionFind, h: &SlotKey) -> bool {
    if tainted.is_empty() {
        return false;
    }
    if tainted.contains(&canon_key_frozen(uf, h)) {
        return true;
    }
    match h {
        SlotKey::Elem(g) => holder_untracked(tainted, uf, g),
        SlotKey::Field(g, f) if !f.lossy().starts_with("__") => holder_untracked(tainted, uf, g),
        _ => false,
    }
}

/// What `seed_untracked_points` found, kept by the width table for
/// lowering's own reads.
#[derive(Default)]
pub(crate) struct PromiseTaint {
    /// The classes holding a promise the walk does not follow.
    tainted: HashSet<SlotKey>,
    /// The value point classes one of whose holders is tainted.
    wide: HashSet<SlotKey>,
    /// The classes that also hold a container.
    held: HashSet<SlotKey>,
}

impl PromiseTaint {
    /// Is the face `f` (a promise's value point, or a number under
    /// one) one a promise the walk does not follow can reach: in a
    /// value point class one of whose holders is tainted, a value
    /// point of a tainted holder, or an element or program-named field
    /// of such a face; and not in a class that also holds a container,
    /// which keeps its form (an async body's result point also carries
    /// the `Error` it rejects with)?
    pub(crate) fn widens(&self, uf: &UnionFind, f: &SlotKey) -> bool {
        !self.tainted.is_empty()
            && !self.held.contains(&canon_key_frozen(uf, f))
            && self.reaches(uf, f)
    }

    fn reaches(&self, uf: &UnionFind, f: &SlotKey) -> bool {
        if self.wide.contains(&canon_key_frozen(uf, f)) {
            return true;
        }
        match f {
            SlotKey::Field(h, n) if n.lossy() == "value" => {
                holder_untracked(&self.tainted, uf, h) || self.reaches(uf, h)
            }
            SlotKey::Elem(g) => self.reaches(uf, g),
            SlotKey::Field(g, n) if !n.lossy().starts_with("__") => self.reaches(uf, g),
            _ => false,
        }
    }
}

/// After `canonicalize`: take the frozen classes of the untracked
/// sources, close them over every alias pair the walk recorded (`alias_pairs`,
/// joined or not: a guarded pair joins only on container evidence, and
/// a promise slot has none, yet the two sides hold the same promise),
/// then seed every number face noted under a promise that
/// [`PromiseTaint::widens`]: only a number's width is at stake, and a
/// value point a slot of another type joins (a handler's `any`
/// parameter, an iterator step's `value`, a container) must keep its
/// form. Answers what lowering's own `await` reads ask; nothing in a
/// program that has no promise to read.
pub(super) fn seed_untracked_points(
    a: &mut Analysis,
    alias_pairs: &[(SlotKey, SlotKey)],
) -> PromiseTaint {
    if !a.promises {
        return PromiseTaint::default();
    }
    // a slot the desugar spelled carries what the compiler routes
    // through it, in the form it was handed (`escape_propagate`)
    let sources = (a.any_seeds.iter())
        .chain(&a.escape_only_seeds)
        .chain(&a.untracked_values)
        .filter(|k| !super::escape_propagate::synthetic(k));
    let mut tainted: HashSet<SlotKey> = sources.map(|k| canon_key_frozen(&a.uf, k)).collect();
    if tainted.is_empty() {
        return PromiseTaint::default();
    }
    close_over_pairs(&mut tainted, &a.uf, alias_pairs);
    let wide = (a.uf.keys())
        .filter_map(|k| match k {
            SlotKey::Field(h, n)
                if n.lossy() == "value" && holder_untracked(&tainted, &a.uf, h) =>
            {
                Some(canon_key_frozen(&a.uf, k))
            }
            _ => None,
        })
        .collect();
    let held = (a.containerish.iter())
        .map(|k| canon_key_frozen(&a.uf, k))
        .collect();
    let taint = PromiseTaint {
        tainted,
        wide,
        held,
    };
    let points: Vec<SlotKey> = (a.number_faces.iter())
        .filter(|f| taint.widens(&a.uf, f))
        .map(|f| canon_key_frozen(&a.uf, f))
        .collect();
    a.seeds.extend(points);
    taint
}

/// The canonical keys whose class taints `k`: its own, and those of
/// the arrays and program-named fields holding it.
fn holder_chain(uf: &UnionFind, k: &SlotKey) -> Vec<SlotKey> {
    let mut out = vec![canon_key_frozen(uf, k)];
    let mut h = k;
    while let SlotKey::Elem(g) | SlotKey::Field(g, _) = h {
        if matches!(h, SlotKey::Field(_, f) if f.lossy().starts_with("__")) {
            break;
        }
        out.push(canon_key_frozen(uf, g));
        h = g;
    }
    out
}

/// Close `tainted` over the alias pairs: a pair one side of which is
/// held by a tainted class taints the other side's class. Each pair is
/// filed under every key whose taint reaches it, so each newly tainted
/// class is visited once.
fn close_over_pairs(tainted: &mut HashSet<SlotKey>, uf: &UnionFind, pairs: &[(SlotKey, SlotKey)]) {
    let mut by_holder: HashMap<SlotKey, Vec<SlotKey>> = HashMap::new();
    for (x, y) in pairs {
        for (from, to) in [(x, y), (y, x)] {
            let to = canon_key_frozen(uf, to);
            for c in holder_chain(uf, from) {
                by_holder.entry(c).or_default().push(to.clone());
            }
        }
    }
    let mut work: Vec<SlotKey> = tainted.iter().cloned().collect();
    while let Some(k) = work.pop() {
        for to in by_holder.get(&k).into_iter().flatten() {
            if tainted.insert(to.clone()) {
                work.push(to.clone());
            }
        }
    }
}
