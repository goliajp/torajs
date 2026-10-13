//! Expression-position `yield` in a CONDITIONALLY evaluated operand —
//! a short-circuit right-hand side (`&&` / `||` / `??`) or a ternary
//! branch (rotation 636; the `yield_expr_hoist` module doc lists these
//! as the positions the plain hoist had to refuse).
//!
//! The plain hoist moves the yield in front of its statement, which
//! would run it UNCONDITIONALLY. Here the operand's hoisted statements
//! go inside a branch of their own, and the whole operator folds into
//! one `__yx_<n>` temp the statement then reads:
//!
//! ```text
//! a && (yield b)   →  let t = a; if (t) { <hoisted>; t = b' }         … t
//! a || (yield b)   →  let t = a; if (!t) { … }
//! a ?? (yield b)   →  let t = a; if (t == null) { … }
//! c ? X : Y        →  let t; if (c) { <hX>; t = X' } else { <hY>; t = Y' }
//! ```
//!
//! The left operand / condition is evaluated into the temp AHEAD of
//! the statement, in the same buffer slot the hoisted yields of any
//! earlier operand already took (buffer order is source order). The
//! temp carries the `__yx_` prefix on purpose: `check_hoist_eval_order`
//! then treats the read like every other hoisted-yield temp, so a
//! side effect the source ordered before the operator is still
//! refused rather than reordered.

use super::*;

/// Which short-circuit operator reaches its right-hand side.
#[derive(Clone, Copy)]
pub(super) enum ShortCircuit {
    /// `&&` — the rhs runs when the lhs is truthy.
    And,
    /// `||` — the rhs runs when the lhs is falsy.
    Or,
    /// `??` — the rhs runs when the lhs is `null` or `undefined`.
    Nullish,
}

impl<'a> Parser<'a> {
    /// `left OP right` where `right` hoisted `hoisted` (non-empty):
    /// the operator's value as a temp, its rhs-side yields inside the
    /// branch the operator would take.
    pub(super) fn short_circuit_yield_temp(
        &mut self,
        op: ShortCircuit,
        left: ExprId,
        hoisted: Vec<Stmt>,
        right: ExprId,
    ) -> ExprId {
        let temp = self.mint_yield_temp(left);
        let read = self.ast.add_expr(Expr::Ident(temp.clone()));
        let cond = match op {
            ShortCircuit::And => read,
            ShortCircuit::Or => self.ast.add_expr(Expr::Unary {
                op: crate::ast::UnaryOp::Not,
                expr: read,
            }),
            ShortCircuit::Nullish => {
                let null = self.ast.add_expr(Expr::Null);
                self.ast.add_expr(Expr::BinOp {
                    op: BinOp::LooseEq,
                    left: read,
                    right: null,
                })
            }
        };
        let then_branch = self.temp_assign_block(&temp, hoisted, right);
        self.yield_hoist_buf.push(Stmt::If {
            cond,
            then_branch: Box::new(then_branch),
            else_branch: None,
        });
        self.ast.add_expr(Expr::Ident(temp))
    }

    /// `cond ? then : else` where at least one branch hoisted: the
    /// ternary's value as a temp, each branch's yields inside its own
    /// arm of an `if`.
    pub(super) fn ternary_yield_temp(
        &mut self,
        cond: ExprId,
        then: (Vec<Stmt>, ExprId),
        els: (Vec<Stmt>, ExprId),
    ) -> ExprId {
        let uninit = self.ast.add_expr(Expr::Uninit);
        let temp = self.mint_yield_temp(uninit);
        let then_branch = self.temp_assign_block(&temp, then.0, then.1);
        let else_branch = self.temp_assign_block(&temp, els.0, els.1);
        self.yield_hoist_buf.push(Stmt::If {
            cond,
            then_branch: Box::new(then_branch),
            else_branch: Some(Box::new(else_branch)),
        });
        self.ast.add_expr(Expr::Ident(temp))
    }

    /// A loop condition or step, `A, B, …`, whose yields run where
    /// their operand does. The whole list hoisted into one buffer put
    /// every yield in front of `A`, so `i++, yield i` yielded the value
    /// before the increment (§13.16.1 evaluates the operands left to
    /// right). Each operand captures its own yields instead, and the
    /// operands before one that yields become a statement in front of
    /// its yields: `A, yield B, C` answers `[A; <hB>]` and `B', C`.
    /// Without a yield the list is the plain sequence and the
    /// statements are empty.
    pub(super) fn parse_loop_clause(&mut self) -> Result<(ExprId, Vec<Stmt>), String> {
        let mut before = Vec::new();
        let mut rest: Option<ExprId> = None;
        loop {
            let (e, hoisted) = self.with_yield_hoist_captured(|p| p.parse_expr())?;
            if !hoisted.is_empty() {
                before.extend(rest.take().map(Stmt::Expr));
                before.extend(hoisted);
            }
            rest = Some(match rest {
                Some(left) => self.ast.add_expr(Expr::Sequence { left, right: e }),
                None => e,
            });
            if !matches!(self.peek(), Token::Comma) {
                break;
            }
            self.pos += 1;
        }
        Ok((rest.expect("the loop parsed an operand"), before))
    }

    /// A `for` test or step, which `empty` says the source left out.
    pub(super) fn parse_opt_loop_clause(
        &mut self,
        empty: bool,
    ) -> Result<(Option<ExprId>, Vec<Stmt>), String> {
        if empty {
            return Ok((None, Vec::new()));
        }
        let (e, hoisted) = self.parse_loop_clause()?;
        Ok((Some(e), hoisted))
    }

    /// A fresh `__yx_<n>` temp declared in the hoist buffer with `init`.
    fn mint_yield_temp(&mut self, init: ExprId) -> String {
        let id = self.mint_desugar_id();
        let temp = format!("__yx_{id}");
        self.yield_hoist_buf.push(Stmt::LetDecl {
            mutable: true,
            name: temp.clone(),
            // The branches may answer different types, and the
            // yield's resumption value is always any.
            type_ann: Some("any".into()),
            init,
            is_var: false,
        });
        temp
    }

    /// `{ <hoisted>; temp = value }`.
    fn temp_assign_block(&mut self, temp: &str, hoisted: Vec<Stmt>, value: ExprId) -> Stmt {
        let target = self.ast.add_expr(Expr::Ident(temp.to_string()));
        let assign = self.ast.add_expr(Expr::Assign { target, value });
        let mut block = hoisted;
        block.push(Stmt::Expr(assign));
        Stmt::Block(block)
    }
}
