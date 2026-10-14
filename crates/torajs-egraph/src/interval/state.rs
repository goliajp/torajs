//! The analysis state: one slot per value of the function, so reading
//! a value's current fact is an index rather than a hash probe.

use crate::id_hash::HashMap;

use torajs_core::ssa::ValueId;

use super::{AbsVal, NumFact};

pub(super) struct State {
    /// `None`: the value has no entry yet (distinct from `Bottom`)
    slots: Vec<Option<AbsVal>>,
}

impl State {
    pub(super) fn new(values: usize) -> Self {
        State {
            slots: vec![None; values],
        }
    }

    pub(super) fn get(&self, v: &ValueId) -> Option<&AbsVal> {
        self.slots.get(v.0 as usize)?.as_ref()
    }

    pub(super) fn insert(&mut self, v: ValueId, val: AbsVal) {
        self.slots[v.0 as usize] = Some(val);
    }

    /// The values whose state is a fact.
    pub(super) fn into_facts(self) -> HashMap<ValueId, NumFact> {
        self.slots
            .into_iter()
            .enumerate()
            .filter_map(|(i, s)| match s {
                Some(AbsVal::Fact(f)) => Some((ValueId(i as u32), f)),
                _ => None,
            })
            .collect()
    }
}
