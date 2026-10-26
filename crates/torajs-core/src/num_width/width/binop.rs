//! The width of a binary operator's result, for [`Analysis::width_of`].

use super::super::{Analysis, Scope, W, join, literal_is_f64};
use crate::ast::{BinOp, Expr, ExprId, UnaryOp};

/// Mark every slot dependency as reaching the consumer through a
/// growth op (W5). Inside an assignment-graph cycle that marking
/// makes the cycle non-i64-safe.
fn mark_growth(w: W) -> W {
    match w {
        W::Num(deps) => W::Num(deps.into_iter().map(|(k, _)| (k, true)).collect()),
        other => other,
    }
}

impl<'a> Analysis<'a> {
    /// [`Self::width_of`]'s BinOp arm — per-operator width facing.
    pub(super) fn width_of_binop(
        &self,
        op: BinOp,
        left: ExprId,
        right: ExprId,
        scope: &Scope,
    ) -> W {
        match op {
            BinOp::Div | BinOp::Pow => W::F64,
            // ToInt32 firewall: bitwise / shift results are int32
            // regardless of operand width (JS spec §13.9 / §13.12).
            BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::UShr => W::Num(Vec::new()),
            BinOp::Lt
            | BinOp::Gt
            | BinOp::Le
            | BinOp::Ge
            | BinOp::Eq
            | BinOp::Neq
            | BinOp::LooseEq
            | BinOp::LooseNeq => W::NotNum,
            BinOp::Mul => self.width_of_mul(left, right, scope),
            // Add/Sub with a literal int increment stays a linear
            // small-step counter (`i = i + 1`) — passes through
            // unmarked. A non-constant increment can be any size
            // per step, so it marks growth. Whether the sum leaves
            // the exact window is the magnitude pass's question.
            BinOp::Add | BinOp::Sub => {
                let w = join(self.width_of(left, scope), self.width_of(right, scope));
                if self.is_int_const(left, scope) || self.is_int_const(right, scope) {
                    w
                } else {
                    mark_growth(w)
                }
            }
            // W3 — `%` can mint -0 at runtime (negative dividend
            // with zero remainder), unrepresentable in i64: any
            // slot the result reaches must stay f64. A plain
            // integer-literal dividend is non-negative (negation
            // is Unary), bounding the remainder in [+0, |b|) — it
            // keeps the pass-through join, where growth still
            // flows through (the intermediate `(n*3+1) % m`
            // diverges before the mod contracts it).
            //
            // srem runtime-0 (§5.3 follow-up close) — the divisor
            // must ALSO be a provably non-zero literal: `7 % b`
            // with a runtime-zero b is NaN per spec (the lowering
            // carve mirrors this; aarch64 sdiv-by-zero silently
            // handed the dividend back).
            BinOp::Mod => {
                let dividend_ok = matches!(
                    self.ast.get_expr(left),
                    Expr::Number(n) if !literal_is_f64(*n)
                );
                let divisor_ok = matches!(
                    self.ast.get_expr(right),
                    Expr::Number(m) if *m != 0.0 && !literal_is_f64(*m)
                );
                if dividend_ok && divisor_ok {
                    join(self.width_of(left, scope), self.width_of(right, scope))
                } else {
                    W::F64
                }
            }
            BinOp::LAnd | BinOp::LOr => {
                join(self.width_of(left, scope), self.width_of(right, scope))
            }
        }
    }

    /// Mul grows multiplicatively — any slot feeding it is
    /// a growth dependency (W5). In a cycle that means
    /// geometric blow-up past 2^53 within tens of steps.
    ///
    /// W3 C4 + S9 — int `a * b` mints -0 when one factor
    /// is zero and the other negative; a non-positive
    /// integer-literal cofactor or two non-literal factors
    /// (runtime zero × runtime negative) make that
    /// reachable, so the result must stay f64 (mirrors the
    /// lower_binop float predicate). A positive literal
    /// cofactor or a literal pair missing the zero×negative
    /// pattern keeps the int face. A square (`x * x`) cannot
    /// mint -0 but is past 2^53 once `x` passes 2^26.5, which
    /// a counter reaches in well under a second; it keeps the
    /// int face only as a compare operand, which no slot
    /// receives (`ssa_lower_binop::square`).
    fn width_of_mul(&self, left: ExprId, right: ExprId, scope: &Scope) -> W {
        let lit = |eid: ExprId| -> Option<i64> {
            match self.ast.get_expr(eid) {
                Expr::Number(n) if !literal_is_f64(*n) => Some(*n as i64),
                Expr::Unary {
                    op: UnaryOp::Neg,
                    expr,
                } => match self.ast.get_expr(*expr) {
                    Expr::Number(n) if !literal_is_f64(*n) => Some(-(*n as i64)),
                    _ => None,
                },
                _ => None,
            }
        };
        let minus_zero_risk = match (lit(left), lit(right)) {
            (Some(x), Some(y)) => (x == 0 && y < 0) || (x < 0 && y == 0),
            (Some(c), None) | (None, Some(c)) => c <= 0,
            (None, None) => true,
        };
        if minus_zero_risk {
            W::F64
        } else {
            mark_growth(join(
                self.width_of(left, scope),
                self.width_of(right, scope),
            ))
        }
    }
}
