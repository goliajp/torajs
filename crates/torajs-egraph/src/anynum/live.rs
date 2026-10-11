//! Backward liveness over a function: the values live into each
//! block. A side exit writes back what its target reads.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Function, ValueId};

use crate::float_demote::cfg::{successors, term_operand_value};
use crate::rc_peephole::visit_value_operands;

pub(super) fn live_in(func: &Function) -> HashMap<BlockId, HashSet<ValueId>> {
    let mut live_in: HashMap<BlockId, HashSet<ValueId>> = HashMap::default();
    let mut changed = true;
    while changed {
        changed = false;
        for block in func.blocks.iter().rev() {
            let mut live: HashSet<ValueId> = HashSet::default();
            for s in successors(&block.term) {
                if let Some(li) = live_in.get(&s) {
                    live.extend(li.iter().copied());
                }
            }
            live.extend(term_operand_value(&block.term));
            for inst in block.insts.iter().rev() {
                if let Some(r) = inst.result {
                    live.remove(&r);
                }
                visit_value_operands(&inst.kind, |v| {
                    live.insert(v);
                });
            }
            let entry = live_in.entry(block.id).or_default();
            if *entry != live {
                *entry = live;
                changed = true;
            }
        }
    }
    live_in
}
