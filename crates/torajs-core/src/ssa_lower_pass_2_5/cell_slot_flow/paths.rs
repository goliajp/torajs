//! The paths a cell takes into a slot of another static type, and
//! the openness they carry.
//!
//! A cell of a layout fits a slot of any prefix of it, and once it
//! sits there it is read back typed as the prefix: if the wider
//! layout's slots can hold cells of unknown layout, so can the
//! prefix's. Every place a cell changes static type has to reach
//! [`Paths::widen`] (through `Scan::lays`) for that to hold; one that
//! does not is a prefix slot left closed while it holds a foreign
//! cell.
//!
//! A slot that holds a cell is itself many slots: its fields and, for
//! an array, its elements. Two static types over one cell type each
//! of those twice, and either view writes what the other reads, so a
//! field or element whose cell type differs between the two is a
//! crossing in both directions: every layout at the bottom of either
//! side opens ([`Paths::align`]).

use crate::ssa::id_hash::HashSet;

use super::is_prefix;
use crate::ssa::{Module, StructId, Type};

#[derive(Default)]
pub(super) struct Paths {
    /// `(wide, narrow)`: a cell of `wide` reached a slot of `narrow`,
    /// a prefix of it.
    widens: HashSet<(StructId, StructId)>,
    /// Layout pairs whose fields were already aligned.
    aligned: HashSet<(StructId, StructId)>,
    /// Layouts a differing field or element opened outright.
    forced: HashSet<StructId>,
}

impl Paths {
    /// A cell of `wide` reaches a slot of `narrow`, a prefix of it.
    pub(super) fn widen(&mut self, module: &Module, wide: StructId, narrow: StructId) {
        if self.widens.insert((wide, narrow)) {
            self.align_fields(module, wide, narrow);
        }
    }

    /// Every field the two layouts share, by index: the same slot
    /// under each one's declared type.
    fn align_fields(&mut self, module: &Module, a: StructId, b: StructId) {
        let layouts = &module.struct_layouts;
        let (la, lb) = (&layouts[a.0 as usize], &layouts[b.0 as usize]);
        for ((_, ta), (_, tb)) in la.iter().zip(lb.iter()) {
            self.align(module, *ta, *tb);
        }
    }

    /// One slot under two declared types `x` and `y`: when they
    /// differ, each view stores what the other reads.
    pub(super) fn align(&mut self, module: &Module, x: Type, y: Type) {
        if x == y {
            return;
        }
        let (a, b) = (elem_obj(module, x), elem_obj(module, y));
        self.forced.extend(a);
        self.forced.extend(b);
        if let (Some(a), Some(b)) = (a, b)
            && a != b
            && self.aligned.insert((a, b))
        {
            self.align_fields(module, a, b);
        }
    }

    /// Every `(wide, narrow)` path recorded.
    pub(super) fn widens(&self) -> Vec<(StructId, StructId)> {
        let mut v: Vec<_> = self.widens.iter().copied().collect();
        v.sort_by_key(|(w, n)| (w.0, n.0));
        v
    }

    /// Close `open` over the recorded paths.
    pub(super) fn settle(self, open: &mut [bool]) {
        for sid in self.forced {
            open[sid.0 as usize] = true;
        }
        let mut changed = true;
        while changed {
            changed = false;
            for &(wide, narrow) in &self.widens {
                if open[wide.0 as usize] && !open[narrow.0 as usize] {
                    open[narrow.0 as usize] = true;
                    changed = true;
                }
            }
        }
    }

    /// A function an indirect call, a table or the runtime enters is
    /// handed whatever its callers' static types say, which nothing
    /// here compares with its own: each cell parameter takes a cell of
    /// every layout extending its own, and each array parameter an
    /// array of every such layout, whose elements are then slots under
    /// both element types ([`Paths::align`]).
    pub(super) fn entered_blind(&mut self, module: &Module, param: Type) {
        let Some(narrow) = elem_obj(module, param) else {
            return;
        };
        let layouts = &module.struct_layouts;
        for wide in 0..layouts.len() {
            let w = StructId(wide as u32);
            if w == narrow || !is_prefix(&layouts[narrow.0 as usize], &layouts[wide]) {
                continue;
            }
            match param {
                Type::Arr(_) => self.align(module, Type::Obj(narrow), Type::Obj(w)),
                _ => self.widen(module, w, narrow),
            }
        }
    }
}

/// The cell layout at the bottom of a (nested) array type.
pub(in crate::ssa_lower_pass_2_5) fn elem_obj(module: &Module, ty: Type) -> Option<StructId> {
    match ty {
        Type::Obj(sid) => Some(sid),
        Type::Arr(a) => elem_obj(module, module.arr_layouts[a.0 as usize]),
        _ => None,
    }
}
