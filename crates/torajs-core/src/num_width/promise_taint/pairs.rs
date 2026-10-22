//! The closure of the untracked classes over the alias pairs, for
//! [`super::seed_untracked_points`].

use super::super::container::{UnionFind, canon_key_frozen};
use super::super::{KeySet, SlotKeyData, SlotKeyId, SlotKeys};

/// File `to` under every class whose taint reaches `k`: its own, and
/// those of the arrays and program-named fields holding it. Answers
/// `k`'s own class.
fn file(
    uf: &UnionFind,
    keys: &SlotKeys,
    k: SlotKeyId,
    to: SlotKeyId,
    edges: &mut Vec<(SlotKeyId, SlotKeyId)>,
) -> SlotKeyId {
    let own = canon_key_frozen(uf, keys, k);
    edges.push((own, to));
    let mut h = k;
    loop {
        match keys.data(h) {
            SlotKeyData::Elem(g) => h = g,
            SlotKeyData::Field(g, f) if !keys.dunder(f) => h = g,
            _ => return own,
        }
        edges.push((canon_key_frozen(uf, keys, h), to));
    }
}

/// Close `tainted` over the alias pairs: a pair one side of which is
/// held by a tainted class taints the other side's class. Each pair is
/// filed under every class whose taint reaches it, so each newly
/// tainted class is visited once.
pub(super) fn close_over_pairs<'p>(
    tainted: &mut KeySet,
    uf: &UnionFind,
    keys: &SlotKeys,
    pairs: impl Iterator<Item = &'p (SlotKeyId, SlotKeyId)>,
) {
    // (class, the class of a pair side its taint reaches)
    let mut edges: Vec<(SlotKeyId, SlotKeyId)> = Vec::with_capacity(4 * pairs.size_hint().0);
    for &(x, y) in pairs {
        let cy = canon_key_frozen(uf, keys, y);
        let cx = file(uf, keys, x, cy, &mut edges);
        file(uf, keys, y, cx, &mut edges);
    }
    edges.sort_unstable();
    // by class: where its edges start in `edges`
    let n = keys.len();
    let mut start = vec![0usize; n + 1];
    for &(c, _) in &edges {
        start[c.index() + 1] += 1;
    }
    for i in 1..start.len() {
        start[i] += start[i - 1];
    }
    let mut marked = vec![false; n];
    let mut work: Vec<SlotKeyId> = tainted.iter().copied().collect();
    for k in &work {
        marked[k.index()] = true;
    }
    while let Some(i) = work.pop() {
        for &(_, t) in &edges[start[i.index()]..start[i.index() + 1]] {
            if !marked[t.index()] {
                marked[t.index()] = true;
                tainted.insert(t);
                work.push(t);
            }
        }
    }
}
