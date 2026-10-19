//! The reach closure's graph in the form it walks: each expanded
//! node's out-edges decoded once per link, the visited set as bits,
//! and each undefined name resolved once per closure.
//!
//! An edge to a node of the same member depends only on the member
//! bytes, so it is decoded at the first expansion and kept for every
//! later closure of the link (the elide fix-point runs several). An
//! edge through an undefined name stays a name: whether a user fn, a
//! dyld import or a required member answers it is the closure's own
//! question, asked once per name per closure.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::archive::ArMember;
use crate::dead_strip_reach::{MemberReach, Node};
use crate::dead_strip_reach_cache::BytesKey;
use crate::dead_strip_reach_relocs::RelocIndex;
use crate::dead_strip_reach_resolve::{RelocTarget, decode_reloc};

/// One out-edge; `Undef` carries the id `EdgeIndex::name` reads back.
#[derive(Clone, Copy)]
pub(crate) enum Edge {
    /// `atom: None` is the whole section.
    Local {
        sect: u8,
        atom: Option<u32>,
    },
    Undef(u32),
}

#[derive(Default)]
pub(crate) struct EdgeIndex<'a> {
    relocs: RelocIndex<'a>,
    /// Keyed by member bytes, section ordinal and atom (`None` = the
    /// whole section).
    lists: HashMap<(BytesKey<'a>, u8, Option<u32>), Rc<[Edge]>>,
    ids: HashMap<&'a str, u32>,
    names: Vec<&'a str>,
}

impl<'a> EdgeIndex<'a> {
    /// The out-edges of `atom` of section `sect` (`None` = every reloc
    /// of the section), in reloc-table order.
    pub(crate) fn out_edges(
        &mut self,
        member: &ArMember<'a>,
        r: &MemberReach<'a>,
        sect: u8,
        atom: Option<u32>,
    ) -> Result<Rc<[Edge]>, String> {
        let key = (BytesKey(member.data), sect, atom);
        if let Some(list) = self.lists.get(&key) {
            return Ok(list.clone());
        }
        let sa = r.sects.get(sect as usize - 1).ok_or("bad sect")?;
        // r_address is section-relative; atom ranges are object-file
        // addresses.
        let (lo, hi) = match atom {
            Some(a) => {
                let (s, e) = sa.atoms[a as usize];
                (s - sa.addr, e - sa.addr)
            }
            None => (0, sa.size),
        };
        let mut list = Vec::new();
        for e in self.relocs.in_range(member, sect, lo, hi)? {
            match decode_reloc(sect, e, member, r) {
                Some(RelocTarget::Local { sect, atom }) => list.push(Edge::Local { sect, atom }),
                Some(RelocTarget::Undef(name)) => {
                    let next = self.names.len() as u32;
                    let id = *self.ids.entry(name).or_insert(next);
                    if id == next {
                        self.names.push(name);
                    }
                    list.push(Edge::Undef(id));
                }
                None => {}
            }
        }
        let list: Rc<[Edge]> = list.into();
        self.lists.insert(key, list.clone());
        Ok(list)
    }

    pub(crate) fn name(&self, id: u32) -> &'a str {
        self.names[id as usize]
    }
}

/// One closure's answer per undefined-name id, filled on first ask.
#[derive(Default)]
pub(crate) struct UndefMemo {
    answers: Vec<Option<Option<Node>>>,
}

impl UndefMemo {
    pub(crate) fn get_or(
        &mut self,
        id: u32,
        resolve: impl FnOnce() -> Option<Node>,
    ) -> Option<Node> {
        let i = id as usize;
        if i >= self.answers.len() {
            self.answers.resize(i + 1, None);
        }
        *self.answers[i].get_or_insert_with(resolve)
    }
}

/// The closure's visited set: one bit per atom and per whole section
/// of the members it walks.
pub(crate) struct Seen {
    keys: Vec<(usize, usize)>,
    /// Per member, per section: where its atoms start in `atoms`.
    base: Vec<Vec<usize>>,
    atoms: Vec<bool>,
    whole: Vec<Vec<bool>>,
}

impl Seen {
    pub(crate) fn new(reach: &BTreeMap<(usize, usize), MemberReach<'_>>) -> Self {
        let mut seen = Seen {
            keys: reach.keys().copied().collect(),
            base: Vec::with_capacity(reach.len()),
            atoms: Vec::new(),
            whole: Vec::with_capacity(reach.len()),
        };
        let mut n = 0;
        for r in reach.values() {
            seen.base.push(
                r.sects
                    .iter()
                    .map(|sa| {
                        let at = n;
                        n += sa.atoms.len();
                        at
                    })
                    .collect(),
            );
            seen.whole.push(vec![false; r.sects.len()]);
        }
        seen.atoms = vec![false; n];
        seen
    }

    /// Mark `node` visited; true on its first visit. A node outside
    /// the walked members and sections is never recorded — expanding
    /// it does nothing, so a repeat costs nothing.
    pub(crate) fn insert(&mut self, node: Node) -> bool {
        let (key, sect, atom) = match node {
            Node::Atom { key, sect, atom } => (key, sect, Some(atom)),
            Node::AllSect { key, sect } => (key, sect, None),
        };
        let (Ok(m), Some(s)) = (
            self.keys.binary_search(&key),
            (sect as usize).checked_sub(1),
        ) else {
            return true;
        };
        let bit = match atom {
            Some(a) => self.base[m].get(s).map(|&at| &mut self.atoms[at + a]),
            None => self.whole[m].get_mut(s),
        };
        bit.is_none_or(|b| !std::mem::replace(b, true))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::Seen;
    use crate::dead_strip_reach::{MemberReach, Node};
    use crate::dead_strip_reach_table::test_support::sections_reach;

    fn member(atoms_per_sect: &[usize]) -> MemberReach<'static> {
        let starts: Vec<Vec<u64>> = atoms_per_sect
            .iter()
            .map(|&n| (0..n as u64).map(|i| i * 8).collect())
            .collect();
        let sects: Vec<(u64, u64, &[u64])> = starts
            .iter()
            .map(|s| (0, s.len() as u64 * 8, s.as_slice()))
            .collect();
        sections_reach(&sects)
    }

    #[test]
    fn each_node_is_new_once_and_members_do_not_share_bits() {
        let reach = BTreeMap::from([((0, 3), member(&[2, 3])), ((1, 0), member(&[1]))]);
        let mut seen = Seen::new(&reach);
        let atom = |key, sect, atom| Node::Atom { key, sect, atom };
        assert!(seen.insert(atom((0, 3), 2, 2)));
        assert!(!seen.insert(atom((0, 3), 2, 2)));
        assert!(seen.insert(atom((0, 3), 1, 1)));
        assert!(seen.insert(atom((1, 0), 1, 0)));
        assert!(!seen.insert(atom((1, 0), 1, 0)));
        let whole = Node::AllSect {
            key: (0, 3),
            sect: 2,
        };
        assert!(seen.insert(whole));
        assert!(!seen.insert(whole));
        assert!(!seen.insert(atom((0, 3), 2, 2)));
    }

    #[test]
    fn nodes_outside_the_walked_sections_are_never_recorded() {
        let reach = BTreeMap::from([((0, 0), member(&[1]))]);
        let mut seen = Seen::new(&reach);
        for node in [
            Node::AllSect {
                key: (0, 0),
                sect: 0,
            },
            Node::AllSect {
                key: (0, 0),
                sect: 9,
            },
            Node::AllSect {
                key: (5, 5),
                sect: 1,
            },
        ] {
            assert!(seen.insert(node));
            assert!(seen.insert(node));
        }
    }
}
