//! Where the values of a function are defined and read, and which of
//! them a tail copy renames.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{Function, Operand, Terminator, ValueId, visit_value_operands};

/// Where every value is defined and read, and every block's
/// predecessors, by block.
pub(super) struct Sites {
    defs: HashMap<ValueId, Vec<u32>>,
    uses: HashMap<ValueId, Vec<u32>>,
    preds: HashMap<u32, Vec<u32>>,
}

impl Sites {
    pub(super) fn of(func: &Function) -> Self {
        let mut defs: HashMap<ValueId, Vec<u32>> = HashMap::default();
        let mut uses: HashMap<ValueId, Vec<u32>> = HashMap::default();
        let mut preds: HashMap<u32, Vec<u32>> = HashMap::default();
        for block in &func.blocks {
            let id = block.id.0;
            for t in super::targets(&block.term) {
                preds.entry(t).or_default().push(id);
            }
            for inst in &block.insts {
                if let Some(r) = inst.result {
                    defs.entry(r).or_default().push(id);
                }
                visit_value_operands(&inst.kind, |v| uses.entry(v).or_default().push(id));
            }
            match &block.term {
                Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                }
                | Terminator::Ret(Some(Operand::Value(v))) => {
                    uses.entry(*v).or_default().push(id);
                }
                _ => {}
            }
        }
        Sites { defs, uses, preds }
    }

    /// The values that live only inside `tail`: every def and every
    /// read in its blocks, and along the tail a def before each read
    /// (a read ahead of the def is the previous trip's value, carried
    /// around the loop through the join). A tail block entered from
    /// outside the tail as well starts with nothing defined: a path
    /// through that edge runs the original's reads without its earlier
    /// defs, and would read a value only a copy had last written.
    pub(super) fn local_to(&self, func: &Function, tail: &[u32]) -> Vec<ValueId> {
        let in_tail = |blocks: &[u32]| blocks.iter().all(|b| tail.contains(b));
        let mut defined: HashSet<ValueId> = HashSet::default();
        let mut visible: HashSet<ValueId> = HashSet::default();
        let mut carried: HashSet<ValueId> = HashSet::default();
        for (k, &b) in tail.iter().enumerate() {
            if k > 0 && self.preds[&b].iter().any(|&p| p != tail[k - 1]) {
                visible.clear();
            }
            for inst in &func.blocks[b as usize].insts {
                visit_value_operands(&inst.kind, |v| {
                    if !visible.contains(&v) {
                        carried.insert(v);
                    }
                });
                if let Some(r) = inst.result {
                    defined.insert(r);
                    visible.insert(r);
                }
            }
        }
        let mut local: Vec<ValueId> = defined
            .into_iter()
            .filter(|v| !carried.contains(v))
            .filter(|v| in_tail(&self.defs[v]))
            .filter(|v| self.uses.get(v).is_none_or(|u| in_tail(u)))
            .collect();
        local.sort_by_key(|v| v.0);
        local
    }
}
