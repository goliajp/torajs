//! Which promise value points a promise the walk does not follow can
//! reach (`promise_entry` notes the sources and the number faces):
//! the closure of the sources over the alias classes and pairs, and
//! [`PromiseTaint::widens`], the one predicate the analysis seeds by
//! and lowering's `await` reads ask.

use super::container::{UnionFind, canon_key_frozen};
use super::{Analysis, SlotKey};
use crate::ssa::id_hash::HashSet;
use pairs::close_over_pairs;
use std::sync::OnceLock;

mod pairs;

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
    /// The classes of the noted faces and of the points above them.
    faces: HashSet<SlotKey>,
    /// The value point classes among `faces` one of whose holders is
    /// tainted.
    wide: HashSet<SlotKey>,
    /// The keys the walk found a container in, and their classes, the
    /// classes that keep their form; asked only of a face a promise
    /// reaches.
    containerish: Vec<SlotKey>,
    held: OnceLock<HashSet<SlotKey>>,
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
        if self.tainted.is_empty() {
            return false;
        }
        let c = canon_key_frozen(uf, f);
        self.reaches_from(uf, f, &c) && !self.held(uf).contains(&c)
    }

    fn held(&self, uf: &UnionFind) -> &HashSet<SlotKey> {
        self.held.get_or_init(|| {
            (self.containerish.iter())
                .map(|k| canon_key_frozen(uf, k))
                .collect()
        })
    }

    fn reaches(&self, uf: &UnionFind, f: &SlotKey) -> bool {
        self.reaches_from(uf, f, &canon_key_frozen(uf, f))
    }

    /// [`Self::reaches`] of `f`, whose class is `c`.
    fn reaches_from(&self, uf: &UnionFind, f: &SlotKey, c: &SlotKey) -> bool {
        if self.class_wide(uf, c) {
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

impl PromiseTaint {
    /// Is the class `c` a value point class one of whose holders is
    /// tainted? Answered ahead of time for the classes of the noted
    /// faces; a lowering read keyed outside them (a spelling the walk
    /// never keyed a number face under) is answered here by the same
    /// test over the class's members.
    fn class_wide(&self, uf: &UnionFind, c: &SlotKey) -> bool {
        if self.faces.contains(c) {
            return self.wide.contains(c);
        }
        uf.keys().any(|k| {
            matches!(k, SlotKey::Field(h, n) if n.lossy() == "value"
                && canon_key_frozen(uf, k) == *c
                && holder_untracked(&self.tainted, uf, h))
        })
    }
}

/// After `canonicalize`: take the frozen classes of the untracked
/// sources, close them over every alias pair the walk recorded
/// (guarded and nested, joined or not: a guarded pair joins only on
/// container evidence, and a promise slot has none, yet the two sides
/// hold the same promise),
/// then seed every number face noted under a promise that
/// [`PromiseTaint::widens`]: only a number's width is at stake, and a
/// value point a slot of another type joins (a handler's `any`
/// parameter, an iterator step's `value`, a container) must keep its
/// form. Answers what lowering's own `await` reads ask; nothing in a
/// program that has no promise to read.
pub(super) fn seed_untracked_points(a: &mut Analysis) -> PromiseTaint {
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
    let pairs = a.guarded_unions.iter().chain(&a.nested_unions);
    close_over_pairs(&mut tainted, &a.uf, pairs);
    // only the classes a noted face, or a value point above one, is in
    // can answer a read
    let mut faces: HashSet<SlotKey> = HashSet::default();
    for f in &a.number_faces {
        let mut k = f;
        faces.insert(canon_key_frozen(&a.uf, k));
        while let SlotKey::Elem(g) | SlotKey::Field(g, _) = k {
            faces.insert(canon_key_frozen(&a.uf, g));
            k = g;
        }
    }
    let wide = (a.uf.keys())
        .filter_map(|k| match k {
            SlotKey::Field(h, n) if n.lossy() == "value" => Some((h, canon_key_frozen(&a.uf, k))),
            _ => None,
        })
        .filter(|(h, c)| faces.contains(c) && holder_untracked(&tainted, &a.uf, h))
        .map(|(_, c)| c)
        .collect();
    let taint = PromiseTaint {
        tainted,
        faces,
        wide,
        containerish: a.containerish.iter().cloned().collect(),
        held: OnceLock::new(),
    };
    let points: Vec<SlotKey> = (a.number_faces.iter())
        .filter(|f| taint.widens(&a.uf, f))
        .map(|f| canon_key_frozen(&a.uf, f))
        .collect();
    a.seeds.extend(points);
    taint
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(n: &str) -> SlotKey {
        SlotKey::Local("f".into(), n.into())
    }

    fn point(h: SlotKey) -> SlotKey {
        SlotKey::Field(Box::new(h), "value".into())
    }

    /// A read keyed outside the noted faces answers by its class: `a`'s
    /// value point shares a class with that of the tainted `b`, though
    /// `a` itself is not tainted and no face was noted under either.
    #[test]
    fn unnoted_point_answers_by_its_class() {
        let mut uf = UnionFind::default();
        uf.union(&point(local("a")), &point(local("b")));
        let taint = PromiseTaint {
            tainted: [canon_key_frozen(&uf, &local("b"))].into_iter().collect(),
            ..PromiseTaint::default()
        };
        assert!(taint.widens(&uf, &point(local("a"))));
        assert!(!taint.widens(&uf, &point(local("c"))));
    }
}
