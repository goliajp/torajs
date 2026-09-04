//! ES2022 §13.10 `RelationalExpression : PrivateIdentifier in
//! ShiftExpression` — the ergonomic brand check (`#x in o`). Split
//! verbatim out of `parse_comparison`'s head (rotation 400 file-size
//! clean: `expr_prec.rs` was a registered over-500 debt entry and the
//! [In]-restriction knife would have grown it further). The
//! production is its own grammar arm, so the seam is the grammar's.

use super::*;

impl<'a> Parser<'a> {
    /// `Some(expr)` when the cursor stands on `#name in …`; `None`
    /// leaves the cursor untouched for the ordinary relational chain.
    ///
    /// Only legal lexically inside a class body (§13.1 early error
    /// otherwise). Same synthetic-Call shape as `in`
    /// (`__torajs_priv_in_op(key, obj)` — no new Expr variant for
    /// the recursive walkers), the key a deferred
    /// `__privu_<site>__<raw>` placeholder String that
    /// `resolve_private_refs` rewrites like any member reference.
    pub(super) fn try_parse_private_in(&mut self) -> Result<Option<ExprId>, String> {
        let Token::PrivateIdent(n) = self.peek() else {
            return Ok(None);
        };
        if !matches!(
            self.tokens.get(self.pos + 1).map(|s| &s.token),
            Some(Token::Ident(k)) if k == "in"
        ) {
            return Ok(None);
        }
        let n = n.clone();
        if self.current_class.is_none() {
            return Err(format!(
                "private name `#{n}` is only allowed within a class body (at {})",
                self.at()
            ));
        }
        // §14.7.4 early error — the production only exists with
        // the [In] parameter, so a C-style for-head init
        // (`for (#x in v;;)`) refuses it (a parenthesized head
        // resets [In]; `parse_primary_paren` clears the flag).
        if self.in_for_init {
            // The production only exists with the [In] parameter, so
            // under `[~In]` it is simply not this arm: decline, and
            // the caller's ordinary path refuses the token it cannot
            // use. (A parenthesized head resets [In];
            // `parse_primary_paren` clears the flag.)
            return Ok(None);
        }
        let site = self.ast.private_ref_sites.len();
        let mut stack = self.class_stack.clone();
        stack.reverse(); // innermost first
        self.ast.private_ref_sites.push((n.clone(), stack));
        self.pos += 2; // consume `#name` + `in`
        // §13.10 — the rhs is a ShiftExpression: a BARE arrow
        // (`#f in () => {}` / `#f in x => 1`) is a Syntax Error,
        // while a parenthesized one (`#f in (() => {})`) reaches
        // it through PrimaryExpression and is legal. The paren
        // form never leaves a bare-arrow marker: its arrow parses
        // INSIDE parse_primary_paren's recursion.
        let bare_paren_arrow = matches!(self.peek(), Token::LParen) && self.is_arrow_fn_at_lparen();
        let right_start = self.pos;
        let right = self.parse_shift()?;
        if bare_paren_arrow
            || (matches!(self.ast.get_expr(right), Expr::ArrowFn { .. })
                && !matches!(self.tokens[right_start].token, Token::LParen))
        {
            return Err(format!(
                "the right-hand side of `#{n} in` must be a ShiftExpression — parenthesize the arrow function (at {})",
                self.at()
            ));
        }
        let key = self
            .ast
            .add_expr(Expr::String(format!("__privu_{site}__{n}").into()));
        let callee = self
            .ast
            .add_expr(Expr::Ident("__torajs_priv_in_op".to_string()));
        Ok(Some(self.ast.add_expr(Expr::Call {
            callee,
            args: vec![key, right],
        })))
    }
}
