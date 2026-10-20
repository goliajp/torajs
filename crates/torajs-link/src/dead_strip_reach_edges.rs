//! The reach closure's graph in the form it walks: each expanded
//! node's out-edges decoded once per link, the visited set as bits,
//! and each undefined name resolved once per closure.
//!
//! An edge to a node of the same member depends only on the member
//! bytes, so it is decoded at the first expansion and kept for every
//! later closure of the link (the elide fix-point runs several). An
//! import the runtime image's symbol index holds becomes the row of its
//! name there; any other undefined name stays a name. Whether a user
//! fn, a dyld import or a required member answers either is the
//! closure's own question, asked once per symbol per closure.

use std::collections::HashMap;
use std::rc::Rc;

use crate::archive::ArMember;
use crate::dead_strip_reach::{MemberReach, Node};
use crate::dead_strip_reach_cache::BytesKey;
use crate::dead_strip_reach_relocs::RelocIndex;
use crate::dead_strip_reach_resolve::{RelocTarget, decode_reloc};
use crate::dead_strip_reach_table::{Import, ImportRef};

/// One out-edge.
#[derive(Clone, Copy)]
pub(crate) enum Edge {
    /// `atom: None` is the whole section.
    Local { sect: u8, atom: Option<u32> },
    /// An import, by the row of its name in the image's symbol index.
    Sym(u32),
    /// Any other undefined name, by the id `EdgeIndex::name` reads
    /// back.
    Name(u32),
}

/// One section's decoded out-edges: per atom (empty until the
/// section's first atom is expanded), and the whole section's.
type SectEdges = (Vec<Option<Rc<[Edge]>>>, Option<Rc<[Edge]>>);

#[derive(Default)]
pub(crate) struct EdgeIndex<'a> {
    relocs: RelocIndex<'a>,
    /// The slot in `members` of each member's bytes.
    slots: HashMap<BytesKey<'a>, u32>,
    /// Per member slot, per section.
    members: Vec<Vec<SectEdges>>,
    ids: HashMap<&'a str, u32>,
    names: Vec<&'a str>,
}

impl<'a> EdgeIndex<'a> {
    /// The slot of the edges of `member`, whose tables are `r`.
    pub(crate) fn slot(&mut self, member: &ArMember<'a>, r: &MemberReach<'a>) -> u32 {
        let next = self.members.len() as u32;
        let slot = *self.slots.entry(BytesKey(member.data)).or_insert(next);
        if slot == next {
            self.members.push(vec![(Vec::new(), None); r.sects.len()]);
        }
        slot
    }

    /// The out-edges of `atom` of section `sect` (`None` = every reloc
    /// of the section) of the member in `slot`, in reloc-table order.
    /// `imports` are the member's imports, in nlist order.
    pub(crate) fn out_edges(
        &mut self,
        slot: u32,
        member: &ArMember<'a>,
        r: &MemberReach<'a>,
        imports: &[Import<'a>],
        sect: u8,
        atom: Option<u32>,
    ) -> Result<Rc<[Edge]>, String> {
        let sa = r.sects.get(sect as usize - 1).ok_or("bad sect")?;
        let (atoms, whole) = &self.members[slot as usize][sect as usize - 1];
        let cached = match atom {
            Some(a) => atoms.get(a as usize).and_then(Option::as_ref),
            None => whole.as_ref(),
        };
        if let Some(list) = cached {
            return Ok(list.clone());
        }
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
                Some(RelocTarget::Undef(row, name)) => {
                    let sym = imports
                        .binary_search_by_key(&row, |i| i.row)
                        .ok()
                        .and_then(|k| match imports[k].to {
                            ImportRef::Sym(s) => Some(s),
                            ImportRef::Foreign(_) => None,
                        });
                    list.push(match sym {
                        Some(s) => Edge::Sym(s),
                        None => {
                            let next = self.names.len() as u32;
                            let id = *self.ids.entry(name).or_insert(next);
                            if id == next {
                                self.names.push(name);
                            }
                            Edge::Name(id)
                        }
                    });
                }
                None => {}
            }
        }
        let list: Rc<[Edge]> = list.into();
        let (atoms, whole) = &mut self.members[slot as usize][sect as usize - 1];
        match atom {
            Some(a) => {
                atoms.resize(sa.atoms.len(), None);
                atoms[a as usize] = Some(list.clone());
            }
            None => *whole = Some(list.clone()),
        }
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
    /// The answer for `id`, when asked before.
    pub(crate) fn get(&self, id: u32) -> Option<Option<Node>> {
        self.answers.get(id as usize).copied().flatten()
    }

    pub(crate) fn set(&mut self, id: u32, answer: Option<Node>) {
        let i = id as usize;
        if i >= self.answers.len() {
            self.answers.resize(i + 1, None);
        }
        self.answers[i] = Some(answer);
    }
}

/// The closure's visited set: one bit per atom and per whole section
/// of the members it walks, each member named by its position in the
/// walk.
pub(crate) struct Seen {
    /// Per member, per section: where its atoms start in `atoms`.
    base: Vec<Vec<usize>>,
    atoms: Vec<bool>,
    whole: Vec<Vec<bool>>,
}

impl Seen {
    pub(crate) fn new(members: &[MemberReach<'_>]) -> Self {
        let mut seen = Seen {
            base: Vec::with_capacity(members.len()),
            atoms: Vec::new(),
            whole: Vec::with_capacity(members.len()),
        };
        let mut n = 0;
        for r in members {
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

    /// Mark `node` of walked member `m` visited; true on its first
    /// visit. A section outside the member is never recorded —
    /// expanding it does nothing, so a repeat costs nothing.
    pub(crate) fn insert(&mut self, m: usize, node: Node) -> bool {
        let (sect, atom) = match node {
            Node::Atom { sect, atom, .. } => (sect, Some(atom)),
            Node::AllSect { sect, .. } => (sect, None),
        };
        let Some(s) = (sect as usize).checked_sub(1) else {
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
        let members = [member(&[2, 3]), member(&[1])];
        let mut seen = Seen::new(&members);
        let atom = |key, sect, atom| Node::Atom { key, sect, atom };
        assert!(seen.insert(0, atom((0, 3), 2, 2)));
        assert!(!seen.insert(0, atom((0, 3), 2, 2)));
        assert!(seen.insert(0, atom((0, 3), 1, 1)));
        assert!(seen.insert(1, atom((1, 0), 1, 0)));
        assert!(!seen.insert(1, atom((1, 0), 1, 0)));
        let whole = Node::AllSect {
            key: (0, 3),
            sect: 2,
        };
        assert!(seen.insert(0, whole));
        assert!(!seen.insert(0, whole));
        assert!(!seen.insert(0, atom((0, 3), 2, 2)));
    }

    #[test]
    fn sections_outside_the_member_are_never_recorded() {
        let members = [member(&[1])];
        let mut seen = Seen::new(&members);
        for sect in [0, 9] {
            let node = Node::AllSect { key: (0, 0), sect };
            assert!(seen.insert(0, node));
            assert!(seen.insert(0, node));
        }
    }
}
