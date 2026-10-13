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

    /// A loop condition or step, `A, B, …`, and the statements its
    /// yields hoisted.
    pub(super) fn parse_loop_clause(&mut self) -> Result<(ExprId, Vec<Stmt>), String> {
        self.with_yield_hoist_captured(|p| p.parse_comma_list())
    }

    /// `A, B, …` whose yields run where their operand does. Hoisting
    /// an operand's yields in front of the whole list ran them before
    /// `A`, so `(i++, yield i)` yielded the value before the increment
    /// (§13.16.1 evaluates the operands left to right). The operands
    /// before one that yields are evaluated into a temp in front of its
    /// hoisted statements instead: `A, yield B, C` leaves
    /// `[let t = A; <hB>]` in the hoist buffer and answers `t, B', C`.
    /// The answer stays a comma expression, which is no assignment
    /// target, and the temp read stands where `A` was, so anything the
    /// enclosing statement evaluates before the list still meets the
    /// evaluation-order guard.
    pub(super) fn parse_comma_list(&mut self) -> Result<ExprId, String> {
        let mut rest: Option<ExprId> = None;
        loop {
            let mark = self.yield_hoist_buf.len();
            let e = self.parse_expr()?;
            if self.yield_hoist_buf.len() > mark
                && let Some(left) = rest
            {
                let temp = format!("__yx_{}", self.mint_desugar_id());
                let decl = Stmt::LetDecl {
                    mutable: false,
                    name: temp.clone(),
                    type_ann: None,
                    init: left,
                    is_var: false,
                };
                self.yield_hoist_buf.insert(mark, decl);
                rest = Some(self.ast.add_expr(Expr::Ident(temp)));
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
        Ok(rest.expect("the list parsed an operand"))
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
