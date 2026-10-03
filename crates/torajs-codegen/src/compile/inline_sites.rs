//! The intrinsic calls `compile` opens inline.

use torajs_core::ssa::{InstKind, Module};

use super::arr_push::ArrPush;
use super::substr_drop::SubstrDrop;
use super::{CellOps, StrAppendLit};

/// The intrinsics whose calls `compile` opens inline: an inline arm
/// that does the common case and a slow arm that makes the call. The
/// allocator and the emitter both read it: it decides which call
/// sites are rare.
#[derive(Debug, Clone, Copy, Default)]
pub struct InlineSites {
    /// Object-cell allocation and free ([`CellOps`]).
    pub(crate) cells: Option<CellOps>,
    /// Short-literal append ([`StrAppendLit`]).
    pub(crate) str_append: Option<StrAppendLit>,
    /// Substring release ([`SubstrDrop`]).
    pub(crate) substr_drop: Option<SubstrDrop>,
    /// Typed array push ([`ArrPush`]).
    pub(crate) arr_push: Option<ArrPush>,
}

impl InlineSites {
    pub fn of_module(module: &Module) -> InlineSites {
        InlineSites {
            cells: CellOps::of_module(module),
            str_append: StrAppendLit::of_module(module),
            substr_drop: SubstrDrop::of_module(module),
            arr_push: ArrPush::of_module(module),
        }
    }

    /// Whether the call `kind` reaches its `bl` only on a slow arm.
    pub(crate) fn opens_inline(self, kind: &InstKind) -> bool {
        self.cells.is_some_and(|c| c.inline_class(kind).is_some())
            || self.str_append.is_some_and(|s| s.literal(kind).is_some())
            || self.substr_drop.is_some_and(|d| d.matches(kind))
            || self.arr_push.is_some_and(|p| p.matches(kind))
    }
}
