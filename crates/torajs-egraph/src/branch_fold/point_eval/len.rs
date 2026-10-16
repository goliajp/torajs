//! A length load read through the array's length cell, whose defs are
//! built on the first such read: most evaluators never make one.

use torajs_core::ssa::{Operand, ValueId};

use super::super::accum::cell_bound_after_loop;
use super::{Point, PointEval};
use crate::interval::elem_len::{LenDef, len_cells};
use crate::interval::transfer::NumFact;

impl PointEval<'_> {
    /// The tracked array whose length `r` loads, if any.
    pub(super) fn len_root(&self, r: ValueId) -> Option<ValueId> {
        if !self.len_loads.contains(&r) {
            return None;
        }
        let cells = self.len.get_or_init(|| len_cells(self.func, self.names));
        cells.reads.get(&r).copied()
    }

    /// A length load: the value the root's unique last length def
    /// left, read at that def's point.
    pub(super) fn eval_len(&self, root: ValueId, at: Point) -> Option<NumFact> {
        let sites = self.len.get()?.defs.get(&root)?;
        let points: Vec<Point> = sites.iter().map(|(p, _)| *p).collect();
        let last = self.unique_last_def(&points, at)?;
        let (_, def) = sites.iter().find(|(p, _)| *p == last)?;
        let LenDef::Known(op) = def else {
            return None;
        };
        self.eval_operand(op, last).or_else(|| match op {
            Operand::Value(v) => {
                cell_bound_after_loop(self.func, self.la, self, self.dom, *v, last)
            }
            _ => None,
        })
    }
}
