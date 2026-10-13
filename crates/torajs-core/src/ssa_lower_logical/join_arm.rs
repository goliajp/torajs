//! The arms of a value join — `c ? a : b`, `a ?? b`, `a && b`,
//! `a || b` — as the join's single result slot sees them: which arm
//! hands the slot a reference of its own, and how an arm whose repr
//! differs from the slot's is settled into it.
//!
//! A `string` is two reprs here (`Str`, and the `Substr` view a string
//! index or a split answers), and the two block layouts diverge past
//! the header: a view stored in a `Str` slot, or a `Str` in a `Substr`
//! slot, is read through the wrong layout by every consumer. A join
//! over one of each settles at `Str`, the view arm materialized in its
//! own block (`substr_to_owned`, which keeps the undefined sentinel's
//! identity), so only the path that produced the view pays for it.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BlockId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// True when the arm `eid` hands its join a reference the join
    /// owns: an owned shape ([`Self::expr_owned_shape`]), or a string
    /// index read, which mints a fresh Substr view the expression
    /// holds the only reference to. A join with such an arm is made
    /// owned on every path, so a consumer's one release balances
    /// whichever arm ran. A string literal is a static cell and stays
    /// on the borrow side, where it takes no rc traffic.
    pub(crate) fn join_arm_mints(&self, eid: ExprId) -> bool {
        if self.expr_owned_shape(eid) {
            return true;
        }
        if self.pins.operands.contains_key(&eid) {
            return false;
        }
        // A comma expression answers its right operand, so an arm
        // `(f(), s[i])` mints the view `s[i]` does.
        let mut eid = eid;
        while let Expr::As { expr, .. } | Expr::Sequence { right: expr, .. } =
            self.ast.get_expr(eid)
        {
            eid = *expr;
        }
        matches!(self.ast.get_expr(eid), Expr::Index { obj, .. }
            if matches!(self.expr_types.get(obj), Some(crate::check::Type::String)))
    }

    /// Settle the arm `eid`, lowered to `val` and ending in `*block`,
    /// into a join slot of type `slot_ty`; `*block` and `cur_block`
    /// answer the block the arm now ends in. A view arm joining a
    /// `Str` slot becomes an owned `Str` (see the module doc), and the
    /// answer's flag says the arm now holds a fresh reference of its
    /// own. A view the arm minted is released once copied out; a
    /// borrowed one stays with its owner.
    pub(crate) fn settle_join_arm(
        &mut self,
        block: &mut BlockId,
        eid: ExprId,
        val: Operand,
        slot_ty: Type,
    ) -> (Operand, bool) {
        self.cur_block = *block;
        if slot_ty != Type::Str || self.operand_ty(&val) != Type::Substr {
            return (val, false);
        }
        let owned = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.substr_to_owned, vec![val]),
            Type::Str,
            None,
        );
        if self.join_arm_mints(eid) {
            self.emit_drop_value(val, Type::Substr);
        }
        *block = self.cur_block;
        (Operand::Value(owned), true)
    }
}

/// The slot a join of a `left_ty` arm and a `right_ty` arm wears: one
/// `Str` and one `Substr` view settle at `Str`; anything else keeps
/// `left_ty`, the slot the lowering already sized.
pub(crate) fn join_view_slot(left_ty: Type, right_ty: Type) -> Type {
    match (left_ty, right_ty) {
        (Type::Str, Type::Substr) | (Type::Substr, Type::Str) => Type::Str,
        _ => left_ty,
    }
}

impl LowerCtx<'_> {
    /// W3 S8's rule, stated next door in [`crate::ssa_lower_ternary`]:
    /// a number is ONE type to the checker and TWO widths here, so a
    /// join whose arms land on different widths has to settle at
    /// `f64` rather than let the slot wear whichever arm the lowering
    /// happened to see first. `&&` / `||` reconciled nothing, and an
    /// `f64` operand stored into an `i64` slot is not a truncation
    /// the backend performs quietly -- it refuses outright
    /// (`materialize_operand_gpr called on ... holding Fpr`), so
    /// `xs.length && xs[0]` (an i64 length beside an f64 element) was
    /// not a wrong answer but a compile error. A `Str` / `Substr`
    /// pair settles at `Str` the same way ([`join_view_slot`]).
    ///
    /// Answers the joined slot type plus the rhs converted into it,
    /// and whether that conversion minted the rhs a reference of its
    /// own. The lhs lives in a different block, so its half runs at
    /// its own store site through [`Self::coerce_arm_to_slot`].
    pub(crate) fn join_arm_widths(
        &mut self,
        slot_ty: Type,
        right: ExprId,
        b: Operand,
    ) -> (Operand, Type, bool) {
        let b_ty = self.operand_ty(&b);
        let joined = match (slot_ty, b_ty) {
            (Type::I64, Type::F64) | (Type::F64, Type::I64) => Type::F64,
            _ => join_view_slot(slot_ty, b_ty),
        };
        let (b, minted) = self.coerce_arm_to_slot(right, b, joined);
        (b, joined, minted)
    }

    /// One arm's half of [`Self::join_arm_widths`], emitted in that
    /// arm's own block -- which is the whole reason it is not folded
    /// into the join itself.
    pub(crate) fn coerce_arm_to_slot(
        &mut self,
        eid: ExprId,
        op: Operand,
        slot_ty: Type,
    ) -> (Operand, bool) {
        if slot_ty == Type::F64 && self.operand_ty(&op) == Type::I64 {
            return (self.coerce_to_f64(op), false);
        }
        let mut block = self.cur_block;
        self.settle_join_arm(&mut block, eid, op, slot_ty)
    }

    /// Peek whether check.rs answered Any for the JOIN itself while
    /// the operands stayed typed — §13.13 makes `a && b` one of its
    /// two operands, and with no general union type to spell `A | B`
    /// the checker names that Any. The two peeks beside this one look
    /// at the operands and so miss it, and the slot would then take
    /// the lhs's shape and truncate the rhs into it.
    pub(crate) fn result_is_any(&self, eid: ExprId) -> bool {
        matches!(self.expr_types.get(&eid), Some(crate::check::Type::Any))
    }

    /// Peek whether `right` was typed as Any by check.rs without
    /// lowering it (the right side of `&&` / `||` is evaluated
    /// lazily — lowering it eagerly would emit IR onto the wrong
    /// block). Used to decide whether to widen the result slot to
    /// Any so a non-Any short-circuit value and an Any
    /// continuation share a uniform slot type.
    pub(crate) fn right_is_any(&self, right: ExprId) -> bool {
        matches!(self.expr_types.get(&right), Some(crate::check::Type::Any))
    }
}
