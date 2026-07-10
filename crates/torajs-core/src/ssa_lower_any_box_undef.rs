//! RFC 20260710-optional-undefined-repr C2b — sentinel-aware
//! `(tag, value)` encoding for possibly-undefined refcounted
//! pointers, split out of `ssa_lower_any_box.rs` (file-size limit).
//! The `box_to_any_from_expr` / `lower_to_tag_value_raw` Nullable
//! arms call through here.

use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// RFC 20260710 C2b — sentinel-aware `(tag, value)` encoding for
    /// a possibly-undefined refcounted pointer (Nullable Obj / Arr /
    /// Closure slot value): `tag = 4 + (val == UNDEF_CELL)` lands on
    /// ANY_UNDEF=5 for the sentinel (payload ignored by
    /// `anyv_box_from_pair`) and ANY_HEAP=4 for a live cell (NULL
    /// stays tag 4 — `from_pair` maps a zero heap payload to
    /// ANY_NULL). Pure encoding — no rc traffic, no kind mark; each
    /// caller keeps its arm's own ownership discipline.
    pub(crate) fn heap_slot_tag_value(&mut self, val: Operand) -> (Operand, Operand) {
        let val_ty = self.operand_ty(&val);
        let sentinel = self.f.append_inst(
            self.cur_block,
            InstKind::GlobalRef(crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM.to_string()),
            val_ty,
            None,
        );
        let is_undef = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, val.clone(), Operand::Value(sentinel)),
            Type::Bool,
            None,
        );
        let undef_ext = self.f.append_inst(
            self.cur_block,
            InstKind::ZExtBoolToI64(Operand::Value(is_undef)),
            Type::I64,
            None,
        );
        let tag = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Add,
                Operand::ConstI64(4),
                Operand::Value(undef_ext),
            ),
            Type::I64,
            None,
        );
        (Operand::Value(tag), val)
    }
}
