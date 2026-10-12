//! Loop bodies as bitsets: block layout asks whether a block is in
//! a body for every block of every loop.

/// A set of block ids, one bit per id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockSet {
    bits: Vec<u64>,
    len: usize,
}

impl BlockSet {
    pub(crate) fn of(ids: impl Iterator<Item = u32>) -> Self {
        let mut set = BlockSet {
            bits: Vec::new(),
            len: 0,
        };
        for id in ids {
            set.insert(id);
        }
        set
    }

    pub(crate) fn insert(&mut self, id: u32) {
        let w = id as usize / 64;
        if self.bits.len() <= w {
            self.bits.resize(w + 1, 0);
        }
        let bit = 1u64 << (id % 64);
        self.len += usize::from(self.bits[w] & bit == 0);
        self.bits[w] |= bit;
    }

    pub(crate) fn contains(&self, id: u32) -> bool {
        (self.bits.get(id as usize / 64)).is_some_and(|w| w & (1u64 << (id % 64)) != 0)
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_subset(&self, other: &BlockSet) -> bool {
        let theirs = |i: usize| other.bits.get(i).copied().unwrap_or(0);
        (self.bits.iter().enumerate()).all(|(i, w)| w & !theirs(i) == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_size_and_subsets() {
        let a = BlockSet::of([3, 70, 3, 0].into_iter());
        assert_eq!(a.len(), 3, "a repeated id counts once");
        assert!(a.contains(0) && a.contains(3) && a.contains(70));
        assert!(!a.contains(1) && !a.contains(64) && !a.contains(500));
        let mut b = BlockSet::of([0, 3].into_iter());
        assert!(b.is_subset(&a) && !a.is_subset(&b));
        b.insert(200);
        assert!(!b.is_subset(&a), "a member past the other's last word");
        assert!(BlockSet::of(std::iter::empty()).is_subset(&a));
    }
}
