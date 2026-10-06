//! What the interval analysis reads off a function's defs before it
//! iterates.

use torajs_core::ssa::{Function, InstKind, ValueId};

use crate::id_hash::{HashMap, HashSet};

/// The def shapes: single-def values with their defs (what constraint
/// pattern-matching reads), the multi-def cells, and per block the
/// cells it writes (what kills a constraint).
pub(super) type DefShapes = (
    HashMap<ValueId, InstKind>,
    HashSet<ValueId>,
    Vec<HashSet<ValueId>>,
);

pub(super) fn def_shapes(func: &Function) -> DefShapes {
    let mut single: HashMap<ValueId, InstKind> = HashMap::default();
    let mut cells: HashSet<ValueId> = HashSet::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result
                && single.insert(r, inst.kind.clone()).is_some()
            {
                cells.insert(r);
            }
        }
    }
    for v in &cells {
        single.remove(v);
    }
    let mut block_defs: Vec<HashSet<ValueId>> = vec![HashSet::default(); func.blocks.len()];
    for (bi, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            if let Some(r) = inst.result
                && cells.contains(&r)
            {
                block_defs[bi].insert(r);
            }
        }
    }
    (single, cells, block_defs)
}
