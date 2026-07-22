//! Call-arg admit helpers at the SSA scalar ↔ AnyBox boundary,
//! split from `ssa_lower_any_box.rs` (rotation 185 file-size cut).
//! Body is byte-for-byte preserved from the source.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// RFC 20260707 chunk 626 — call-arg admit station. When the
    /// callee's param slot is `Arr<Any>` and the arg's own SSA type
    /// is a typed array (T-11 container widen at the call boundary),
    /// mark the block's elem kind so the callee's kind-aware
    /// `Arr<Any>` readers can decode the raw layout. No-op when the
    /// arg is already `Arr<Any>` (chain 0), boxed `Any`, or not an
    /// array — `emit_arr_mark_kind` self-gates on the value's type.

    /// Chunk 641 — contextual empty-array-literal call arg. An empty
    /// `[]` has no element to infer from; when the callee's param is
    /// a typed `Arr(T)`, alloc the empty block with the PARAM's
    /// layout (mirror of `lower_let_init_val`'s V3-06 empty-literal
    /// annotation arm) instead of the default `Arr<Any>` — the
    /// checker's `empty_lit_into_arr` admit pairs with this so a
    /// FLAG_ARR_ANY block never lands behind a typed param slot
    /// (raw typed writes into NaN-box slots misdecode, chunk 614
    /// family). Returns None for non-empty / non-array-param shapes;
    /// the caller falls through to the plain `lower_expr`.
    pub(crate) fn try_lower_empty_array_arg(
        &mut self,
        arg: crate::ast::ExprId,
        expected: Option<&Type>,
    ) -> Option<Operand> {
        let Some(Type::Arr(aid)) = expected else {
            return None;
        };
        if !matches!(
            self.ast.get_expr(arg),
            crate::ast::Expr::Array(els) if els.is_empty()
        ) {
            return None;
        }
        let ty = Type::Arr(*aid);
        let alloc_fn = if self.arr_layouts[aid.0 as usize] == Type::Any {
            self.intrinsics.arr_alloc_any
        } else {
            self.intrinsics.arr_alloc
        };
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(alloc_fn, vec![Operand::ConstI64(0)]),
            ty,
            None,
        );
        Some(Operand::Value(v))
    }
}
