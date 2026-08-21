//! The element-store coercion of `a[i] = v` (`coerce_elem_store`),
//! carved out of the parent under the 500-line file discipline when
//! the `(Str, Substr)` arm pushed it to 501 (rotation 468). The table
//! answers what the array slot stores for the value's type, and
//! whether that stored value is a transfer.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// W4 — align the stored value with the element's width, and
    /// answer whether the slot now takes the value's own reference.
    ///
    /// The reverse width direction (an f64 value into an i64 element)
    /// means the container width analysis missed a write site, and is
    /// loud rather than bit-punned.
    ///
    /// An `any` rhs is the same crossing the binding and the
    /// assignment boundaries decode (`ssa_lower_stmt_let_decl`'s
    /// scalar row, `ssa_lower_assign_ident`'s coercion table). Without
    /// it the NaN-box bits land in the slot and read back as the
    /// element: `a[0] = v` with `v: any` holding 3 answered NaN, and a
    /// member-shaped source answered the raw box.
    pub(super) fn coerce_elem_store(
        &mut self,
        elem_ty: Type,
        value: ExprId,
        v: Operand,
        transfers: bool,
    ) -> (Operand, bool) {
        match (elem_ty, self.operand_ty(&v)) {
            (Type::F64, Type::I64) => (self.coerce_to_f64(v), transfers),
            (Type::I64, Type::F64) => panic!(
                "ssa-lower: f64 value into i64 array elem — \
                 container width analysis missed this write"
            ),
            (Type::I64 | Type::F64, Type::Any) => {
                // ToNumber only READS the box, and the decoded slot is
                // Copy, so the source's own stake needs settling here.
                let n = self.coerce_any_to_number(v.clone(), elem_ty);
                self.release_owned_temp(value, &v);
                (n, true)
            }
            (Type::Str, Type::Any) => {
                // ToString mints a fresh owned Str — the slot takes
                // exactly that reference, so this is a transfer.
                let s = self.coerce_to_str(v.clone(), Type::Any);
                self.release_owned_temp(value, &v);
                (s, true)
            }
            // A substring VIEW into a Str slot is stored as an owned
            // copy — a view does not leave the block that owns its
            // storage (rotation 468; `a[0] = s[1]` read back garbage
            // once the view's parent block went). The fresh copy
            // transfers; a fresh-mint view is released here, a borrow
            // stays with its owner — the push arm's rule.
            (Type::Str, Type::Substr) => {
                let owned = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.substr_to_owned, vec![v.clone()]),
                    Type::Str,
                    None,
                );
                if transfers {
                    self.emit_drop_value(v, Type::Substr);
                }
                (Operand::Value(owned), true)
            }
            _ => (v, transfers),
        }
    }
}
