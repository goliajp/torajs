//! Where the values of a function are defined and read, and which of
//! them a tail copy renames.

use crate::id_hash::HashSet;

use torajs_core::ssa::{Function, Operand, Terminator, ValueId, visit_value_operands};

/// Where the values a tail defines are defined and read, and every
/// block's predecessors, by block.
pub(super) struct Sites {
    /// Per value index, its row in `defs` and `uses` plus one; 0 for a
    /// value no tail defines, which no copy renames.
    row: Vec<u32>,
    defs: Vec<Vec<u32>>,
    uses: Vec<Vec<u32>>,
    preds: Vec<Vec<u32>>,
}

impl Sites {
    /// The sites of the values the blocks of `tails` define. Only those
    /// may be renamed, and they are few: tracking every value of a big
    /// function cost a map entry per operand.
    pub(super) fn of<'t>(func: &Function, tails: impl Iterator<Item = &'t [u32]>) -> Self {
        let mut row = vec![0u32; func.values.len()];
        let mut rows = 0;
        for &b in tails.flatten() {
            for r in func.blocks[b as usize]
                .insts
                .iter()
                .filter_map(|i| i.result)
            {
                if row[r.0 as usize] == 0 {
                    rows += 1;
                    row[r.0 as usize] = rows;
                }
            }
        }
        let mut defs: Vec<Vec<u32>> = vec![Vec::new(); rows as usize];
        let mut uses: Vec<Vec<u32>> = vec![Vec::new(); rows as usize];
        let mut preds: Vec<Vec<u32>> = vec![Vec::new(); func.blocks.len()];
        let at = |v: ValueId| (row[v.0 as usize] as usize).checked_sub(1);
        for block in &func.blocks {
            let id = block.id.0;
            for t in super::targets(&block.term) {
                preds[t as usize].push(id);
            }
            for inst in &block.insts {
                if let Some(k) = inst.result.and_then(at) {
                    defs[k].push(id);
                }
                visit_value_operands(&inst.kind, |v| {
                    if let Some(k) = at(v) {
                        uses[k].push(id);
                    }
                });
            }
            match &block.term {
                Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                }
                | Terminator::Ret(Some(Operand::Value(v))) => {
                    if let Some(k) = at(*v) {
                        uses[k].push(id);
                    }
                }
                _ => {}
            }
        }
        Sites {
            row,
            defs,
            uses,
            preds,
        }
    }

    /// The row of a value a tail defines.
    fn row(&self, v: ValueId) -> usize {
        self.row[v.0 as usize] as usize - 1
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
            if k > 0 && self.preds[b as usize].iter().any(|&p| p != tail[k - 1]) {
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
            .filter(|&v| in_tail(&self.defs[self.row(v)]))
            .filter(|&v| in_tail(&self.uses[self.row(v)]))
            .collect();
        local.sort_by_key(|v| v.0);
        local
    }
}
