//! The closure of the untracked classes over the alias pairs, for
//! [`super::seed_untracked_points`].

use super::super::SlotKey;
use super::super::container::UnionFind;
use crate::ssa::id_hash::{HashMap, HashSet};

/// The alias classes the pairs' sides and their holders are in, as
/// dense ids: the sides share most of their holders, so each spelling
/// is canonicalized once and each class kept once.
#[derive(Default)]
struct Classes<'p> {
    of: HashMap<&'p SlotKey, usize>,
    ids: HashMap<SlotKey, usize>,
    keys: Vec<SlotKey>,
}

impl<'p> Classes<'p> {
    /// The id of `canon_key_frozen(uf, k)`.
    fn id(&mut self, uf: &UnionFind, k: &'p SlotKey) -> usize {
        if let Some(&i) = self.of.get(k) {
            return i;
        }
        let c = match k {
            SlotKey::Elem(x) => {
                let xi = self.id(uf, x);
                uf.find_frozen(&SlotKey::Elem(Box::new(self.keys[xi].clone())))
            }
            SlotKey::Field(x, n) => {
                let xi = self.id(uf, x);
                uf.find_frozen(&SlotKey::Field(Box::new(self.keys[xi].clone()), n.clone()))
            }
            _ => uf.find_frozen(k),
        };
        let i = match self.ids.get(&c) {
            Some(&i) => i,
            None => {
                self.keys.push(c.clone());
                self.ids.insert(c, self.keys.len() - 1);
                self.keys.len() - 1
            }
        };
        self.of.insert(k, i);
        i
    }

    /// The classes whose taint reaches `k`: its own, and those of the
    /// arrays and program-named fields holding it.
    fn holder_chain(&mut self, uf: &UnionFind, k: &'p SlotKey) -> Vec<usize> {
        let mut out = Vec::new();
        let mut h = k;
        loop {
            out.push(self.id(uf, h));
            match h {
                SlotKey::Elem(g) => h = g,
                SlotKey::Field(g, f) if !f.lossy().starts_with("__") => h = g,
                _ => return out,
            }
        }
    }
}

/// Close `tainted` over the alias pairs: a pair one side of which is
/// held by a tainted class taints the other side's class. Each pair is
/// filed under every class whose taint reaches it, so each newly
/// tainted class is visited once.
pub(super) fn close_over_pairs<'p>(
    tainted: &mut HashSet<SlotKey>,
    uf: &UnionFind,
    pairs: impl Iterator<Item = &'p (SlotKey, SlotKey)>,
) {
    let mut cls = Classes::default();
    // by class: the classes of the pair sides its taint reaches
    let mut taints: Vec<Vec<usize>> = Vec::new();
    for (x, y) in pairs {
        let (cx, cy) = (cls.holder_chain(uf, x), cls.holder_chain(uf, y));
        taints.resize(cls.keys.len(), Vec::new());
        for &c in &cx {
            taints[c].push(cy[0]);
        }
        for &c in &cy {
            taints[c].push(cx[0]);
        }
    }
    let mut marked = vec![false; cls.keys.len()];
    let mut work: Vec<usize> = (tainted.iter())
        .filter_map(|k| cls.ids.get(k).copied())
        .collect();
    for &i in &work {
        marked[i] = true;
    }
    while let Some(i) = work.pop() {
        for &t in &taints[i] {
            if !marked[t] {
                marked[t] = true;
                tainted.insert(cls.keys[t].clone());
                work.push(t);
            }
        }
    }
}
