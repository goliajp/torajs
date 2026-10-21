//! One reach closure's own state: the visited set, and its answer per
//! undefined name the edges carry.

use crate::dead_strip_reach::{MemberReach, Node};

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
