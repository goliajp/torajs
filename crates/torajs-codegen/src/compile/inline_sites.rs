//! The intrinsic calls `compile` opens inline.

use torajs_core::ssa::{InstKind, Module};

pub(super) mod promise_drop;
pub(super) mod str_append_substr;

use super::arr_push::ArrPush;
use super::substr_drop::SubstrDrop;
use super::{CellOps, StrAppendLit};
use promise_drop::PromiseDrop;
use str_append_substr::StrAppendSubstr;

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
    /// Substring append ([`StrAppendSubstr`]).
    pub(crate) str_append_substr: Option<StrAppendSubstr>,
    /// Substring release ([`SubstrDrop`]).
    pub(crate) substr_drop: Option<SubstrDrop>,
    /// Typed array push ([`ArrPush`]).
    pub(crate) arr_push: Option<ArrPush>,
    /// Typed promise release ([`PromiseDrop`]).
    pub(crate) promise_drop: Option<PromiseDrop>,
}

impl InlineSites {
    pub fn of_module(module: &Module) -> InlineSites {
        InlineSites {
            cells: CellOps::of_module(module),
            str_append: StrAppendLit::of_module(module),
            str_append_substr: StrAppendSubstr::of_module(module),
            substr_drop: SubstrDrop::of_module(module),
            arr_push: ArrPush::of_module(module),
            promise_drop: PromiseDrop::of_module(module),
        }
    }

    /// Whether the call `kind` is a substring append opened inline
    /// whose field loads take a spilled view at frame offset `slot`
    /// straight off SP.
    pub(crate) fn folds_substr_view(self, kind: &InstKind, slot: u32) -> bool {
        self.str_append_substr.is_some_and(|s| s.matches(kind))
            && slot <= str_append_substr::VIEW_REACH
    }

    /// Whether the call `kind` reaches its `bl` only on a slow arm.
    pub(crate) fn opens_inline(self, kind: &InstKind) -> bool {
        self.cells.is_some_and(|c| c.inline_class(kind).is_some())
            || self.str_append.is_some_and(|s| s.literal(kind).is_some())
            || self.str_append_substr.is_some_and(|s| s.matches(kind))
            || self.substr_drop.is_some_and(|d| d.matches(kind))
            || self.arr_push.is_some_and(|p| p.matches(kind))
            || self.promise_drop.is_some_and(|d| d.matches(kind))
    }
}

/// Open an inline site at a call that saves `saves` around it: a cell
/// allocation or free served by the thread cache, an append that lands
/// in the accumulator's slack (a short literal or a substring), a push
/// into spare capacity, a substring or promise release that only takes
/// a count off. Each arm stores and reloads `saves` on its own slow path.
pub(super) fn try_emit_saved(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<crate::reloc::Reloc>,
    func: &torajs_core::ssa::Function,
    inst: &torajs_core::ssa::Inst,
    alloc: &crate::regalloc::Assignment,
    saves: &[(crate::reg::Reg, u32)],
    facts: &super::ModuleFacts,
) -> bool {
    let sites = facts.inline;
    sites
        .cells
        .is_some_and(|c| super::cell::try_emit(bytes, relocs, inst, alloc, saves, c))
        || sites
            .str_append
            .is_some_and(|op| super::str_append::try_emit(bytes, relocs, inst, alloc, saves, op))
        || sites
            .str_append_substr
            .is_some_and(|op| str_append_substr::try_emit(bytes, relocs, inst, alloc, saves, op))
        || super::arr_push::try_emit(bytes, relocs, func, inst, alloc, saves, facts)
        || sites
            .substr_drop
            .is_some_and(|op| super::substr_drop::try_emit(bytes, relocs, inst, alloc, saves, op))
        || sites
            .promise_drop
            .is_some_and(|op| promise_drop::try_emit(bytes, relocs, inst, alloc, saves, op))
}
