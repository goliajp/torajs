//! Which promise value points a promise the walk does not follow can
//! reach (`promise_entry` notes the sources and the number faces):
//! the closure of the sources over the alias classes and pairs, and
//! [`PromiseTaint::widens`], the one predicate the analysis seeds by
//! and lowering's `await` reads ask.

use super::container::{UnionFind, canon_key_frozen};
use super::{Analysis, KeySet, SlotKeyData, SlotKeyId, SlotKeys};
use pairs::close_over_pairs;
use std::sync::OnceLock;

mod pairs;

/// Is the holder `h` of a promise value point in an untracked class:
/// reached itself, or the element or program-named field of one that
/// is?
fn holder_untracked(tainted: &KeySet, uf: &UnionFind, keys: &SlotKeys, h: SlotKeyId) -> bool {
    if tainted.is_empty() {
        return false;
    }
    if tainted.contains(&canon_key_frozen(uf, keys, h)) {
        return true;
    }
    match keys.data(h) {
        SlotKeyData::Elem(g) => holder_untracked(tainted, uf, keys, g),
        SlotKeyData::Field(g, f) if !keys.dunder(f) => holder_untracked(tainted, uf, keys, g),
        _ => false,
    }
}

/// What `seed_untracked_points` found, kept by the width table for
/// lowering's own reads.
#[derive(Default)]
pub(crate) struct PromiseTaint {
    /// The classes holding a promise the walk does not follow.
    tainted: KeySet,
    /// The classes of the noted faces and of the points above them.
    faces: KeySet,
    /// The value point classes among `faces` one of whose holders is
    /// tainted.
    wide: KeySet,
    /// The keys the walk found a container in, and their classes, the
    /// classes that keep their form; asked only of a face a promise
    /// reaches. The analysis hands its set over once it is done
    /// reading it ([`Self::keep_containers`]).
    containerish: KeySet,
    held: OnceLock<KeySet>,
    /// The value point classes outside `faces` one of whose holders is
    /// tainted, built on the first read keyed outside the faces.
    wide_unnoted: OnceLock<KeySet>,
    /// By the operand of each `await` the analysis noted, the class of
    /// the value point it read (`PromiseReads::awaits`).
    #[cfg(torajs_checks)]
    awaits: crate::ssa::id_hash::HashMap<crate::ast::ExprId, SlotKeyId>,
}

impl PromiseTaint {
    /// Is the face `f` (a promise's value point, or a number under
    /// one) one a promise the walk does not follow can reach: in a
    /// value point class one of whose holders is tainted, a value
    /// point of a tainted holder, or an element or program-named field
    /// of such a face; and not in a class that also holds a container,
    /// which keeps its form (an async body's result point also carries
    /// the `Error` it rejects with)?
    pub(crate) fn widens(&self, uf: &UnionFind, keys: &SlotKeys, f: SlotKeyId) -> bool {
        self.widens_by(uf, keys, f, &self.containerish)
    }

    /// [`Self::widens`], the container keys being `containerish`.
    fn widens_by(
        &self,
        uf: &UnionFind,
        keys: &SlotKeys,
        f: SlotKeyId,
        containerish: &KeySet,
    ) -> bool {
        if self.tainted.is_empty() {
            return false;
        }
        let c = canon_key_frozen(uf, keys, f);
        self.reaches_from(uf, keys, f, c) && !self.held(uf, keys, containerish).contains(&c)
    }

    fn held(&self, uf: &UnionFind, keys: &SlotKeys, containerish: &KeySet) -> &KeySet {
        self.held.get_or_init(|| {
            (containerish.iter())
                .map(|k| canon_key_frozen(uf, keys, *k))
                .collect()
        })
    }

    /// Take over the analysis's container keys, which it no longer
    /// adds to or reads, for lowering's reads.
    pub(super) fn keep_containers(&mut self, containerish: KeySet) {
        self.containerish = containerish;
    }

    fn reaches(&self, uf: &UnionFind, keys: &SlotKeys, f: SlotKeyId) -> bool {
        self.reaches_from(uf, keys, f, canon_key_frozen(uf, keys, f))
    }

    /// [`Self::reaches`] of `f`, whose class is `c`.
    fn reaches_from(&self, uf: &UnionFind, keys: &SlotKeys, f: SlotKeyId, c: SlotKeyId) -> bool {
        if self.class_wide(uf, keys, c) {
            return true;
        }
        match keys.data(f) {
            SlotKeyData::Field(h, n) if keys.is(n, "value") => {
                holder_untracked(&self.tainted, uf, keys, h) || self.reaches(uf, keys, h)
            }
            SlotKeyData::Elem(g) => self.reaches(uf, keys, g),
            SlotKeyData::Field(g, n) if !keys.dunder(n) => self.reaches(uf, keys, g),
            _ => false,
        }
    }
}

impl PromiseTaint {
    /// Is the class `c` a value point class one of whose holders is
    /// tainted? Answered ahead of time for the classes of the noted
    /// faces; a lowering read keyed outside them (a spelling the walk
    /// never keyed a number face under) is answered by the same test,
    /// over every value point once.
    fn class_wide(&self, uf: &UnionFind, keys: &SlotKeys, c: SlotKeyId) -> bool {
        if self.faces.contains(&c) {
            return self.wide.contains(&c);
        }
        let unnoted = self.wide_unnoted.get_or_init(|| {
            wide_points(&self.tainted, uf, keys, |c| !self.faces.contains(&c)).collect()
        });
        unnoted.contains(&c)
    }

    /// Does lowering read the value point the analysis read for the
    /// `await` of `obj`, through one of `holders`? Answers true for an
    /// operand the analysis noted nothing for.
    #[cfg(torajs_checks)]
    pub(crate) fn await_point_agrees(
        &self,
        uf: &UnionFind,
        keys: &SlotKeys,
        obj: crate::ast::ExprId,
        holders: &[SlotKeyId],
    ) -> bool {
        let Some(want) = self.awaits.get(&obj) else {
            return true;
        };
        (holders.iter()).any(|k| canon_key_frozen(uf, keys, keys.field(*k, "value")) == *want)
    }
}

/// The classes `keep` admits of the value points one of whose holders
/// is tainted.
fn wide_points<'u>(
    tainted: &'u KeySet,
    uf: &'u UnionFind,
    keys: &'u SlotKeys,
    keep: impl Fn(SlotKeyId) -> bool + 'u,
) -> impl Iterator<Item = SlotKeyId> + 'u {
    (uf.keys())
        .filter_map(move |k| match keys.data(k) {
            SlotKeyData::Field(h, n) if keys.is(n, "value") => {
                Some((h, canon_key_frozen(uf, keys, k)))
            }
            _ => None,
        })
        .filter(move |(h, c)| keep(*c) && holder_untracked(tainted, uf, keys, *h))
        .map(|(_, c)| c)
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
    #[cfg(torajs_checks)]
    let awaits: crate::ssa::id_hash::HashMap<_, _> = (a.promise_reads.awaits.iter())
        .map(|(obj, k)| {
            let point = a.keys.field(*k, "value");
            (*obj, canon_key_frozen(&a.uf, &a.keys, point))
        })
        .collect();
    let empty = || PromiseTaint {
        #[cfg(torajs_checks)]
        awaits: awaits.clone(),
        ..PromiseTaint::default()
    };
    if !a.promises {
        return empty();
    }
    // a slot the desugar spelled carries what the compiler routes
    // through it, in the form it was handed (`escape_propagate`)
    let sources = (a.any_seeds.iter())
        .chain(&a.escape_only_seeds)
        .chain(&a.untracked_values)
        .filter(|k| !super::escape_propagate::synthetic(&a.keys, **k));
    let mut tainted: KeySet = (sources.map(|k| canon_key_frozen(&a.uf, &a.keys, *k))).collect();
    if tainted.is_empty() {
        return empty();
    }
    let pairs = a.guarded_unions.iter().chain(&a.nested_unions);
    close_over_pairs(&mut tainted, &a.uf, &a.keys, pairs);
    // only the classes a noted face, or a value point above one, is in
    // can answer a read
    let mut faces = KeySet::default();
    for &f in &a.promise_reads.faces {
        let mut k = f;
        faces.insert(canon_key_frozen(&a.uf, &a.keys, k));
        while let Some(g) = a.keys.inner(k) {
            faces.insert(canon_key_frozen(&a.uf, &a.keys, g));
            k = g;
        }
    }
    let wide = wide_points(&tainted, &a.uf, &a.keys, |c| faces.contains(&c)).collect();
    let taint = PromiseTaint {
        tainted,
        faces,
        wide,
        containerish: Default::default(),
        held: OnceLock::new(),
        wide_unnoted: OnceLock::new(),
        #[cfg(torajs_checks)]
        awaits,
    };
    let points: Vec<SlotKeyId> = (a.promise_reads.faces.iter())
        .filter(|f| taint.widens_by(&a.uf, &a.keys, **f, &a.containerish))
        .map(|f| canon_key_frozen(&a.uf, &a.keys, *f))
        .collect();
    a.seeds.extend(points);
    taint
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(keys: &SlotKeys, n: &str) -> SlotKeyId {
        keys.local("f", n)
    }

    fn point(keys: &SlotKeys, h: SlotKeyId) -> SlotKeyId {
        keys.field(h, "value")
    }

    /// A read keyed outside the noted faces answers by its class: `a`'s
    /// value point shares a class with that of the tainted `b`, though
    /// `a` itself is not tainted and no face was noted under either.
    #[test]
    fn unnoted_point_answers_by_its_class() {
        let k = SlotKeys::default();
        let mut uf = UnionFind::default();
        let (pa, pb) = (point(&k, local(&k, "a")), point(&k, local(&k, "b")));
        uf.union(&k, pa, pb);
        let taint = PromiseTaint {
            tainted: [canon_key_frozen(&uf, &k, local(&k, "b"))]
                .into_iter()
                .collect(),
            ..PromiseTaint::default()
        };
        assert!(taint.widens(&uf, &k, pa));
        assert!(!taint.widens(&uf, &k, point(&k, local(&k, "c"))));
    }

    /// The check answers by the value point's class: a key in the class
    /// the analysis read agrees, one in another class does not, and an
    /// operand the analysis noted nothing for agrees with anything.
    #[cfg(torajs_checks)]
    #[test]
    fn await_point_agrees_by_class() {
        let k = SlotKeys::default();
        let mut uf = UnionFind::default();
        let (pa, pb) = (point(&k, local(&k, "a")), point(&k, local(&k, "b")));
        uf.union(&k, pa, pb);
        let obj = crate::ast::ExprId(7);
        let taint = PromiseTaint {
            awaits: [(obj, canon_key_frozen(&uf, &k, pa))].into_iter().collect(),
            ..PromiseTaint::default()
        };
        let (b, c) = (local(&k, "b"), local(&k, "c"));
        assert!(taint.await_point_agrees(&uf, &k, obj, &[b]));
        assert!(!taint.await_point_agrees(&uf, &k, obj, &[c]));
        assert!(taint.await_point_agrees(&uf, &k, crate::ast::ExprId(8), &[c]));
    }
}
