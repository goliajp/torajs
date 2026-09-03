//! The short-circuit rung of the precedence ladder — `??`, `||` and
//! `&&` — split out of `expr_prec` (rotation 572, which pushed that
//! file past 500 while giving `??` the early error §13.14 asks for).
//! The seam is the production itself: these three are the operators
//! whose right operand may not be evaluated at all, and the one rule
//! that spans them is that a `??` may not be mixed with the other two
//! without parentheses.

use super::*;

impl<'a> Parser<'a> {
    /// `lhs ?? rhs` — left-associative, below ternary in precedence.
    /// Lowered as a new `Expr::Nullish { lhs, rhs }` because the lhs
    /// must be evaluated EXACTLY ONCE (it can have side effects); a
    /// pure ternary desugar would either re-evaluate or require an
    /// expression-level `let-binding` we don't have. ssa_lower stores
    /// the lhs into a temp slot and branches on its null-ness.
    pub(super) fn parse_nullish(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_logical_or()?;
        let head_was_logical = self.bare_logical;
        if !self.at_nullish_op() {
            return Ok(left);
        }
        // §13.14 — `CoalesceExpressionHead : CoalesceExpression |
        // BitwiseORExpression`, so `a || b ?? c` and `a && b ?? c` are
        // SyntaxErrors, and so is `a ?? b || c` on the other side. The
        // parenthesised forms are the point of the production: those
        // `||`s were eaten by a nested parse and leave `bare_logical`
        // false here.
        //
        // tr never had this early error. It read as one for a while
        // anyway, because the checker refused `number || string` for
        // its own reasons, and a test asserting "this must not run"
        // cannot tell which rule stopped it -- until the type refusal
        // went (rotation 572) and these two cases said what they had
        // always meant.
        if head_was_logical {
            return Err(
                "`??` cannot be mixed with `||` / `&&` without parentheses (ES 13.14)".into(),
            );
        }
        while self.at_nullish_op() {
            self.pos += 1;
            // The operand is a BitwiseORExpression — reading it at the
            // logical level is what silently accepted `a ?? b || c`.
            // `??` rhs is conditionally evaluated — no yield hoist.
            let right = self.with_yield_hoist_disallowed(|p| p.parse_bit_or())?;
            left = self.ast.add_expr(Expr::Nullish {
                lhs: left,
                rhs: right,
            });
        }
        // `||=` / `&&=` after a `??` is an invalid assignment TARGET,
        // not this production — leave that error to `parse_assign`,
        // which names it properly.
        if matches!(self.peek(), Token::PipePipe | Token::AmpAmp)
            && !matches!(
                self.tokens.get(self.pos + 1).map(|s| &s.token),
                Some(Token::Eq)
            )
        {
            return Err(
                "`??` cannot be mixed with `||` / `&&` without parentheses (ES 13.14)".into(),
            );
        }
        Ok(left)
    }

    /// A `??` that is this operator and not the head of `??=` — the
    /// V3-18 wedge, which leaves the compound assignment to
    /// `parse_assign`.
    fn at_nullish_op(&self) -> bool {
        matches!(self.peek(), Token::QuestionQuestion)
            && !matches!(
                self.tokens.get(self.pos + 1).map(|s| &s.token),
                Some(Token::Eq)
            )
    }

    pub(super) fn parse_logical_or(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_logical_and()?;
        // Captured before the loop: the `&&` answer belongs to this
        // level too, and the rhs parses below will overwrite the flag
        // with their own.
        let mut bare = self.bare_logical;
        // V3-18 wedge — `||=` belongs to parse_assign; decline `||`
        // when `=` follows.
        while matches!(self.peek(), Token::PipePipe)
            && !matches!(
                self.tokens.get(self.pos + 1).map(|s| &s.token),
                Some(Token::Eq)
            )
        {
            self.pos += 1;
            // `||` rhs is conditionally evaluated — no yield hoist.
            let right = self.with_yield_hoist_disallowed(|p| p.parse_logical_and())?;
            left = self.ast.add_expr(Expr::BinOp {
                op: BinOp::LOr,
                left,
                right,
            });
            bare = true;
        }
        self.bare_logical = bare;
        Ok(left)
    }

    pub(super) fn parse_logical_and(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_bit_or()?;
        let mut bare = false;
        // V3-18 wedge — `&&=` belongs to parse_assign; decline `&&`
        // when `=` follows.
        while matches!(self.peek(), Token::AmpAmp)
            && !matches!(
                self.tokens.get(self.pos + 1).map(|s| &s.token),
                Some(Token::Eq)
            )
        {
            self.pos += 1;
            // `&&` rhs is conditionally evaluated — no yield hoist.
            let right = self.with_yield_hoist_disallowed(|p| p.parse_bit_or())?;
            left = self.ast.add_expr(Expr::BinOp {
                op: BinOp::LAnd,
                left,
                right,
            });
            bare = true;
        }
        self.bare_logical = bare;
        Ok(left)
    }
}
