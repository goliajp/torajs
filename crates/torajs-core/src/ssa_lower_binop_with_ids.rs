//! Binop dispatch layer for `LowerCtx<'a>` extracted from `ssa_lower.rs`
//! chunk 397 — Path A.3-batch18.
//!
//! Single method: `lower_binop_with_ids(op, a, b, left_id, right_id)`
//! — sits between the `Expr::BinOp` match arm in
//! `crate::ssa_lower_binop::lower` and the type-flavor algorithm in
//! `crate::ssa_lower_binop_inner::lower`.
//!
//! Responsibilities:
//! - Save/restore `binop.left_undef_id` / `binop.right_undef_id` /
//!   `binop.mul_square` context flags around the inner call so nested
//!   binops don't leak state.
//! - Set `binop.mul_square` when both operand ExprIds resolve to the
//!   same `Ident` (S9 square carve for `x * x`).
//! - Filter `left_id` / `right_id` by frontend `Type::Undefined`
//!   check (P1.5/P1.8 Eq/Neq undef-vs-null distinction).
//!
//! Companion cleanups (0 caller in-tree, dropped alongside the
//! extract):
//!   - `lower_binop(op, a, b)` thin wrapper — dead, no caller ever
//!     passed `None, None`.
//!   - `lower_binop_inner(op, a, b)` — single-call-site wrapper,
//!     inlined directly to `crate::ssa_lower_binop_inner::lower`.

use crate::ast::{BinOp as AstBinOp, Expr, ExprId};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Type-aware BinOp lowering. Decision rule:
    ///   - `/` always produces f64. Both operands coerced to f64. (Use `>>`
    ///     or explicit conversion for integer division — see collatz.tora.ts
    ///     for the convention.)
    ///   - Otherwise: if either operand is f64, both coerced to f64 and
    ///     a float-flavored op is emitted (FAdd/FSub/FMul, FCmp).
    ///   - Bitwise ops + Mod stay integer-only; mixing them with f64 is a
    ///     type error (caught at lower-time, not tolerated).
    /// P1.5/P1.8 — peek a binop operand's source ExprId to see if its
    /// frontend type is Type::Undefined. Set by callers that have
    /// the AST in hand (currently the Eq/Neq path in lower_expr).
    /// None means "no info — treat as null per old behavior". The
    /// pair is `(left_id, right_id)`. Cleared after each lower_binop
    /// call so it doesn't leak across unrelated dispatches.
    pub(crate) fn lower_binop_with_ids(
        &mut self,
        op: AstBinOp,
        a: Operand,
        b: Operand,
        left_id: Option<ExprId>,
        right_id: Option<ExprId>,
    ) -> Operand {
        let saved_left_f64u = self.binop.left_f64_undefable;
        let saved_right_f64u = self.binop.right_f64_undefable;
        self.binop.left_f64_undefable = left_id
            .is_some_and(|eid| crate::ssa_lower_undef_f64_source::is_undef_f64_source(self, eid));
        self.binop.right_f64_undefable = right_id
            .is_some_and(|eid| crate::ssa_lower_undef_f64_source::is_undef_f64_source(self, eid));
        let saved_left_na = self.binop.left_nullable_arr;
        let saved_right_na = self.binop.right_nullable_arr;
        self.binop.left_nullable_arr = left_id
            .is_some_and(|eid| crate::ssa_lower_nullable_guard::is_nullable_arr_source(self, eid));
        self.binop.right_nullable_arr = right_id
            .is_some_and(|eid| crate::ssa_lower_nullable_guard::is_nullable_arr_source(self, eid));
        let saved_left = self.binop.left_undef_id.take();
        let saved_right = self.binop.right_undef_id.take();
        let saved_left_null = self.binop.left_null_id.take();
        let saved_right_null = self.binop.right_null_id.take();
        let saved_square = self.binop.mul_square;
        self.binop.mul_square = matches!(op, AstBinOp::Mul)
            && matches!(
                (
                    left_id.map(|e| self.ast.get_expr(e)),
                    right_id.map(|e| self.ast.get_expr(e)),
                ),
                (Some(Expr::Ident(l)), Some(Expr::Ident(r))) if l == r
            );
        self.binop.left_undef_id = left_id.filter(|eid| {
            matches!(
                self.expr_types.get(eid),
                Some(crate::check::Type::Undefined)
            )
        });
        self.binop.right_undef_id = right_id.filter(|eid| {
            matches!(
                self.expr_types.get(eid),
                Some(crate::check::Type::Undefined)
            )
        });
        self.binop.left_null_id = left_id
            .filter(|eid| matches!(self.expr_types.get(eid), Some(crate::check::Type::Null)));
        self.binop.right_null_id = right_id
            .filter(|eid| matches!(self.expr_types.get(eid), Some(crate::check::Type::Null)));
        let r = crate::ssa_lower_binop_inner::lower(self, op, a, b);
        self.binop.left_nullable_arr = saved_left_na;
        self.binop.right_nullable_arr = saved_right_na;
        self.binop.left_f64_undefable = saved_left_f64u;
        self.binop.right_f64_undefable = saved_right_f64u;
        self.binop.left_undef_id = saved_left;
        self.binop.right_undef_id = saved_right;
        self.binop.left_null_id = saved_left_null;
        self.binop.right_null_id = saved_right_null;
        self.binop.mul_square = saved_square;
        r
    }
}
