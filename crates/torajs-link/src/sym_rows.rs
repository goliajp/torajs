//! Symbols named by their row in the runtime image's symbol index.
//!
//! The image sorts its index by name and every member table lists its
//! imports by row (`dead_strip_reach_table`), so the passes that walk
//! the member closure keep their sets of symbols as bits over those
//! rows: a lookup is one shift, where a name costs a hash or a binary
//! search over long mangled names.

/// A set of rows of one image's symbol index.
pub(crate) struct SymSet {
    words: Vec<u64>,
}

impl SymSet {
    /// The empty set over an index of `n` rows.
    pub(crate) fn new(n: usize) -> Self {
        SymSet {
            words: vec![0; n.div_ceil(64)],
        }
    }

    /// Add `row`; true when it was not in the set.
    pub(crate) fn insert(&mut self, row: u32) -> bool {
        let (w, bit) = (row as usize / 64, 1u64 << (row % 64));
        let fresh = self.words[w] & bit == 0;
        self.words[w] |= bit;
        fresh
    }

    pub(crate) fn contains(&self, row: u32) -> bool {
        self.words[row as usize / 64] >> (row % 64) & 1 == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_is_new_once() {
        let mut s = SymSet::new(130);
        assert!(!s.contains(129));
        assert!(s.insert(129));
        assert!(!s.insert(129));
        assert!(s.contains(129));
        assert!(!s.contains(128));
        assert!(s.insert(0));
        assert!(s.contains(0));
    }
}
