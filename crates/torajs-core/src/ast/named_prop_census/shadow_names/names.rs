//! What one pass of the shadow census collects, by the kind of the
//! value each write stores.

use std::collections::HashSet;

use super::super::accessor_names::Reach;
use super::super::write_hit::{Hit, Kind};

/// The names a pass reaches. `unknown` holds every name written a
/// value that is no number, which may shadow any slot but a number
/// slot; `any`, `string` and `boolean` split it by what the value was.
#[derive(Default)]
pub(super) struct Names {
    pub(super) unknown: HashSet<String>,
    pub(super) number: HashSet<String>,
    pub(super) any: HashSet<String>,
    pub(super) string: HashSet<String>,
    pub(super) boolean: HashSet<String>,
    pub(super) all_unknown: bool,
    pub(super) all_number: bool,
    /// The names a delete reaches, and whether one reaches every name.
    pub(super) deleted: HashSet<String>,
    pub(super) all_deleted: bool,
}

impl Names {
    pub(super) fn take(&mut self, hit: &Hit) {
        match hit {
            Hit::None => {}
            Hit::Name(n, k) => self.name(n, *k),
            Hit::Names(ns) => {
                for (n, k) in ns {
                    self.name(n, *k);
                }
            }
            Hit::All(Kind::Number) => self.all_number = true,
            Hit::All(_) => self.all_unknown = true,
        }
    }

    /// Note what the delete that `hit` names reaches.
    pub(super) fn delete(&mut self, hit: &Hit) {
        match hit {
            Hit::None => {}
            Hit::Name(n, _) => {
                self.deleted.insert(n.to_string());
            }
            Hit::Names(ns) => (self.deleted).extend(ns.iter().map(|(n, _)| n.clone())),
            Hit::All(_) => self.all_deleted = true,
        }
    }

    fn name(&mut self, n: &str, k: Kind) {
        let by_kind = match k {
            Kind::Number => &mut self.number,
            Kind::String => &mut self.string,
            Kind::Boolean => &mut self.boolean,
            Kind::Unknown => &mut self.any,
        };
        by_kind.insert(n.to_string());
        if k != Kind::Number {
            self.unknown.insert(n.to_string());
        }
    }

    pub(super) fn reach(all: bool, names: HashSet<String>) -> Reach {
        match (all, names.is_empty()) {
            (true, _) => Reach::All,
            (false, true) => Reach::None,
            (false, false) => Reach::Names(names),
        }
    }
}
