//! What the interval analysis reads off a function's defs before it
//! iterates.

use torajs_core::ssa::{Function, InstKind, ValueId};

use crate::id_hash::HashSet;

/// The values a function defines exactly once, with their defs, by
/// value id: what constraint pattern-matching reads. A value defined
/// more often (a cell) or never has none.
pub struct SingleDefs<'f> {
    kinds: Vec<Option<&'f InstKind>>,
}

impl<'f> SingleDefs<'f> {
    /// The single-def values of `func`.
    pub fn of(func: &'f Function) -> Self {
        Self::with_cells(func).0
    }

    /// The single-def values of `func`, and per value whether more
    /// than one instruction defines it.
    fn with_cells(func: &'f Function) -> (Self, Vec<bool>) {
        let n = func.values.len();
        let mut kinds: Vec<Option<&'f InstKind>> = vec![None; n];
        let mut multi = vec![false; n];
        for inst in func.blocks.iter().flat_map(|b| &b.insts) {
            if let Some(r) = inst.result {
                let i = r.0 as usize;
                if kinds[i].replace(&inst.kind).is_some() {
                    multi[i] = true;
                }
            }
        }
        for (k, m) in kinds.iter_mut().zip(&multi) {
            if *m {
                *k = None;
            }
        }
        (SingleDefs { kinds }, multi)
    }

    /// The def of `v` when it has exactly one.
    pub fn get(&self, v: &ValueId) -> Option<&'f InstKind> {
        self.kinds.get(v.0 as usize).copied().flatten()
    }

    pub fn contains_key(&self, v: &ValueId) -> bool {
        self.get(v).is_some()
    }
}

/// The def shapes: the single-def values with their defs, the
/// multi-def cells, and per block the cells it writes (what kills a
/// constraint).
pub(super) type DefShapes<'f> = (SingleDefs<'f>, HashSet<ValueId>, Vec<Vec<ValueId>>);

pub(super) fn def_shapes(func: &Function) -> DefShapes<'_> {
    let (single, multi) = SingleDefs::with_cells(func);
    let cells: HashSet<ValueId> = multi
        .iter()
        .enumerate()
        .filter(|(_, m)| **m)
        .map(|(i, _)| ValueId(i as u32))
        .collect();
    let block_defs = func
        .blocks
        .iter()
        .map(|block| {
            let mut defs: Vec<ValueId> = block
                .insts
                .iter()
                .filter_map(|i| i.result)
                .filter(|r| multi[r.0 as usize])
                .collect();
            defs.sort_unstable_by_key(|v| v.0);
            defs.dedup();
            defs
        })
        .collect();
    (single, cells, block_defs)
}
