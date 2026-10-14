//! The members whose typed reads the checker types `any`, and the reads
//! that asked.
//!
//! An any-lane write can leave a member holding a value of another type
//! (`ast::ShadowedNames`); the census needs the checker's types, so a
//! run is seeded with what the run before found, and runs again while
//! the set grows. Only a member read consults the set, so a run whose
//! reads would all answer the same under the grown set types every
//! expression the same: recording which reads asked lets the fixpoint
//! skip that run.

use std::collections::HashSet;

use crate::ast::ShadowedNames;

#[derive(Default)]
pub(crate) struct ShadowedReads {
    /// The names this run retypes.
    pub(crate) names: ShadowedNames,
    /// The members a read asked about, a `number` member's apart.
    asked_number: HashSet<String>,
    asked_other: HashSet<String>,
}

impl ShadowedReads {
    pub(crate) fn seeded(names: ShadowedNames) -> Self {
        ShadowedReads {
            names,
            ..Default::default()
        }
    }

    /// Whether the read of the member `name` (a `number` member when
    /// `number`) types `any`, noting that it asked.
    pub(crate) fn retypes(&mut self, name: &str, number: bool) -> bool {
        // a member the compiler mints never mistypes, whatever grows
        if name.starts_with("__") {
            return false;
        }
        let asked = match number {
            true => &mut self.asked_number,
            false => &mut self.asked_other,
        };
        if !asked.contains(name) {
            asked.insert(name.to_string());
        }
        self.names.may_mistype(name, number)
    }

    /// Whether a run seeded with `more` too would answer some read of
    /// this run otherwise.
    pub(crate) fn would_retype(&self, more: &ShadowedNames) -> bool {
        if more.is_subset(&self.names) {
            return false;
        }
        let mut grown = self.names.clone();
        grown.extend(more.clone());
        let flips = |n: &String, number: bool| {
            grown.may_mistype(n, number) && !self.names.may_mistype(n, number)
        };
        self.asked_number.iter().any(|n| flips(n, true))
            || self.asked_other.iter().any(|n| flips(n, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(unknown: &[&str], number: &[&str]) -> ShadowedNames {
        ShadowedNames {
            unknown: unknown.iter().map(|s| s.to_string()).collect(),
            number: number.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_run_whose_reads_answer_alike_needs_no_successor() {
        let mut r = ShadowedReads::seeded(names(&["a"], &[]));
        assert!(r.retypes("a", true));
        assert!(!r.retypes("b", true));
        assert!(!r.retypes("s", false));
        // a number written into a number member changes no read
        assert!(!r.would_retype(&names(&[], &["b"])));
        // nor does a name no read asked about
        assert!(!r.would_retype(&names(&["z"], &["z"])));
        // a number written into a string member does
        assert!(r.would_retype(&names(&[], &["s"])));
        assert!(r.would_retype(&names(&["b"], &[])));
    }
}
