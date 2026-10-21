//! The closure of the untracked classes over the alias pairs, for
//! [`super::seed_untracked_points`].

use super::super::SlotKey;
use super::super::container::UnionFind;
use crate::ssa::id_hash::{HashMap, HashSet};
use std::borrow::Cow;

/// The alias classes the pairs' sides and their holders are in, as
/// dense ids: each class is kept once, by reference to the pair or the
/// union-find entry spelling it, and copied only when it is tainted.
#[derive(Default)]
struct Classes<'p> {
    /// By the address of a spelling in the pairs: its class id, and
    /// whether the spelling is its class's representative itself (a
    /// key built over it then needs no rewrite before it is resolved).
    /// A holder is shared by the spellings over it, so it is resolved
    /// once; a spelling repeated at another address is resolved again,
    /// to the same class.
    of: HashMap<*const SlotKey, (u32, bool)>,
    ids: HashMap<Cow<'p, SlotKey>, u32>,
    keys: Vec<Cow<'p, SlotKey>>,
}

impl<'p> Classes<'p> {
    /// The id of `canon_key_frozen(uf, k)`, and whether that is `k`.
    fn id(&mut self, uf: &'p UnionFind, k: &'p SlotKey) -> (u32, bool) {
        if let Some(&e) = self.of.get(&(k as *const SlotKey)) {
            return e;
        }
        let rewrite = match k {
            SlotKey::Elem(x) | SlotKey::Field(x, _) => match self.id(uf, x) {
                (_, true) => None,
                (xi, false) => Some(Box::new(self.keys[xi as usize].clone().into_owned())),
            },
            _ => None,
        };
        let e = match rewrite {
            None => match uf.root_of(k) {
                None => (self.intern(Cow::Borrowed(k)), true),
                Some(c) => (self.intern(Cow::Borrowed(c)), false),
            },
            Some(inner) => {
                let r = match k {
                    SlotKey::Field(_, n) => SlotKey::Field(inner, n.clone()),
                    _ => SlotKey::Elem(inner),
                };
                // `r` differs from `k` in its inner key, so only a
                // representative found for it can be `k`
                match uf.root_of(&r) {
                    Some(c) => (self.intern(Cow::Borrowed(c)), c == k),
                    None => (self.intern(Cow::Owned(r)), false),
                }
            }
        };
        self.of.insert(k as *const SlotKey, e);
        e
    }

    fn intern(&mut self, c: Cow<'p, SlotKey>) -> u32 {
        if let Some(&i) = self.ids.get(&*c) {
            return i;
        }
        let i = self.keys.len() as u32;
        self.keys.push(c.clone());
        self.ids.insert(c, i);
        i
    }

    /// File `to` under every class whose taint reaches `k`: its own,
    /// and those of the arrays and program-named fields holding it.
    /// Answers `k`'s own class.
    fn file(
        &mut self,
        uf: &'p UnionFind,
        k: &'p SlotKey,
        to: u32,
        edges: &mut Vec<(u32, u32)>,
    ) -> u32 {
        let own = self.id(uf, k).0;
        edges.push((own, to));
        let mut h = k;
        loop {
            match h {
                SlotKey::Elem(g) => h = g,
                SlotKey::Field(g, f) if !f.lossy().starts_with("__") => h = g,
                _ => return own,
            }
            edges.push((self.id(uf, h).0, to));
        }
    }
}

/// Close `tainted` over the alias pairs: a pair one side of which is
/// held by a tainted class taints the other side's class. Each pair is
/// filed under every class whose taint reaches it, so each newly
/// tainted class is visited once.
pub(super) fn close_over_pairs<'p>(
    tainted: &mut HashSet<SlotKey>,
    uf: &'p UnionFind,
    pairs: impl Iterator<Item = &'p (SlotKey, SlotKey)>,
) {
    // most pairs bring two spellings of their own and a holder or two
    let n = 2 * pairs.size_hint().0;
    let mut cls = Classes::default();
    cls.of.reserve(n);
    cls.ids.reserve(n);
    cls.keys.reserve(n);
    // (class, the class of a pair side its taint reaches)
    let mut edges: Vec<(u32, u32)> = Vec::with_capacity(2 * n);
    for (x, y) in pairs {
        let cy = cls.id(uf, y).0;
        let cx = cls.file(uf, x, cy, &mut edges);
        cls.file(uf, y, cx, &mut edges);
    }
    edges.sort_unstable();
    // by class: where its edges start in `edges`
    let mut start = vec![0usize; cls.keys.len() + 1];
    for &(c, _) in &edges {
        start[c as usize + 1] += 1;
    }
    for i in 1..start.len() {
        start[i] += start[i - 1];
    }
    let mut marked = vec![false; cls.keys.len()];
    let mut work: Vec<u32> = (tainted.iter())
        .filter_map(|k| cls.ids.get(k).copied())
        .collect();
    for &i in &work {
        marked[i as usize] = true;
    }
    while let Some(i) = work.pop() {
        for &(_, t) in &edges[start[i as usize]..start[i as usize + 1]] {
            if !marked[t as usize] {
                marked[t as usize] = true;
                tainted.insert(cls.keys[t as usize].clone().into_owned());
                work.push(t);
            }
        }
    }
}
