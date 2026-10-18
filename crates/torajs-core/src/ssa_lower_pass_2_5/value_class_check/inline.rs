//! The check's reading of arrays that hold a value class in place and
//! of the views (`Type::Inline`) their element reads answer.

use crate::ssa::{ArrId, InstKind, Operand, StructId, Type};

use super::Reader;

#[cfg(test)]
mod tests;

/// Runtime entries an array holding its elements in place may be handed
/// to through a pointer: the grow of an append, the release of an array
/// whose elements carry nothing to release, and the element-kind mark a
/// struct field store writes. None of them reads a slot.
pub(super) const INLINE_ARRAY_ENTRIES: &[&str] = &[
    "__torajs_arr_reserve",
    "__torajs_arr_drop_scalar",
    "__torajs_arr_mark_kind",
];

/// Runtime entries a view may be handed to: the nullish test of a field
/// read's receiver, which compares the word and never reads through it.
pub(super) const VIEW_ENTRIES: &[&str] = &["__torajs_heap_nullish_check"];

impl Reader<'_> {
    pub(super) fn holds_in_place(&self, a: ArrId) -> bool {
        matches!(self.module.arr_layouts[a.0 as usize], Type::Inline(_))
    }

    /// A view of an element of a value class.
    pub(super) fn view(&self, o: &Operand) -> Option<StructId> {
        self.held(o)
            .filter(|(_, t)| matches!(t, Type::Inline(_)))
            .map(|(s, _)| s)
    }

    /// NULL, or the address of a named global: what a view may be told
    /// apart from.
    fn sentinel(&self, o: &Operand) -> bool {
        match o {
            Operand::ConstPtrNull => true,
            Operand::Value(v) => self.globals.contains(v),
            _ => false,
        }
    }

    /// A view compared with anything but a sentinel, or stored anywhere
    /// but a local slot of its own type.
    pub(super) fn view_inst(&self, kind: &InstKind) -> Vec<(StructId, String)> {
        let hit = match kind {
            InstKind::ICmp(_, a, b) => (self.view(a).filter(|_| !self.sentinel(b)))
                .or_else(|| self.view(b).filter(|_| !self.sentinel(a)))
                .map(|s| (s, "compared")),
            InstKind::Store(v, slot, _) => self
                .view(v)
                .filter(|s| match slot {
                    Operand::Value(p) => self.allocas.get(p) != Some(&Type::Inline(*s)),
                    _ => true,
                })
                .map(|s| (s, "stored")),
            InstKind::StoreDyn(v, _, _) | InstKind::StoreDynScaled8(v, _, _) => {
                self.view(v).map(|s| (s, "stored"))
            }
            _ => None,
        };
        hit.map(|(s, what)| (s, format!("a view {what}: {kind:?}")))
            .into_iter()
            .collect()
    }
}
