//! Expression precedence ladder (chunk 408).
//!
//! Pratt-style precedence chain, extracted verbatim from parser.rs:
//! parse_ternary → parse_nullish → parse_logical_or → parse_logical_and →
//! parse_bit_or → parse_bit_xor → parse_bit_and → parse_equality →
//! parse_comparison → parse_shift → parse_additive → parse_multiplicative →
//! parse_pow → parse_unary. Sits between `parse_assign` (main file, calls
//! `parse_ternary` for the RHS of `=`) and `parse_postfix`
//! (`parser/parse_postfix.rs`, called by `parse_unary` fallthrough).
//!
//! All 14 methods `pub(super)` for cross-module impl-block access:
//! `parse_ternary` is invoked from `parse_assign` in the main file;
//! internal chain calls also cross module boundary via `self.*`.
//! Body unchanged.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_ternary(&mut self) -> Result<ExprId, String> {
        let cond = self.parse_nullish()?;
        if !matches!(self.peek(), Token::Question) {
            return Ok(cond);
        }
        self.pos += 1;
        // Ternary branches evaluate conditionally — expression-
        // position yield may not hoist out of them.
        // §13.13 — the THEN branch is AssignmentExpression[+In]
        // whatever the surrounding context, so a for-head init's
        // [~In] restriction lifts here; the ELSE branch inherits
        // [?In] and keeps it (the relational `in` arm refuses there).
        // A yield in either branch folds the ternary into a temp whose
        // arms run the branch's yields (`yield_cond_hoist`).
        let saved_in_for_init = std::mem::replace(&mut self.in_for_init, false);
        let (then_branch, then_hoisted) = self.with_yield_hoist_captured(|p| p.parse_assign())?;
        self.in_for_init = saved_in_for_init;
        if !matches!(self.peek(), Token::Colon) {
            return Err(format!(
                "expected `:` in ternary expression, got {:?}",
                self.peek()
            ));
        }
        self.pos += 1;
        let (else_branch, else_hoisted) = self.with_yield_hoist_captured(|p| p.parse_assign())?;
        if then_hoisted.is_empty() && else_hoisted.is_empty() {
            return Ok(self.ast.add_expr(Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            }));
        }
        Ok(self.ternary_yield_temp(
            cond,
            (then_hoisted, then_branch),
            (else_hoisted, else_branch),
        ))
    }

    pub(super) fn parse_bit_or(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_bit_xor()?;
        // V3-18 wedge — `|=` belongs to parse_assign; decline `|` when
        // `=` follows.
        while matches!(self.peek(), Token::Pipe)
            && !matches!(
                self.tokens.get(self.pos + 1).map(|s| &s.token),
                Some(Token::Eq)
            )
        {
            self.pos += 1;
            let right = self.parse_bit_xor()?;
            left = self.ast.add_expr(Expr::BinOp {
                op: BinOp::BitOr,
                left,
                right,
            });
        }
        Ok(left)
    }

    pub(super) fn parse_bit_xor(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_bit_and()?;
        // V3-18 wedge — `^=` belongs to parse_assign.
        while matches!(self.peek(), Token::Caret)
            && !matches!(
                self.tokens.get(self.pos + 1).map(|s| &s.token),
                Some(Token::Eq)
            )
        {
            self.pos += 1;
            let right = self.parse_bit_and()?;
            left = self.ast.add_expr(Expr::BinOp {
                op: BinOp::BitXor,
                left,
                right,
            });
        }
        Ok(left)
    }

    pub(super) fn parse_bit_and(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_equality()?;
        // V3-18 wedge — `&=` belongs to parse_assign.
        while matches!(self.peek(), Token::Amp)
            && !matches!(
                self.tokens.get(self.pos + 1).map(|s| &s.token),
                Some(Token::Eq)
            )
        {
            self.pos += 1;
            let right = self.parse_equality()?;
            left = self.ast.add_expr(Expr::BinOp {
                op: BinOp::BitAnd,
                left,
                right,
            });
        }
        Ok(left)
    }

    pub(super) fn parse_equality(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_comparison()?;
        loop {
            let op = match self.peek() {
                Token::EqEqEq => BinOp::Eq,
                Token::BangEqEq => BinOp::Neq,
                Token::EqEq => BinOp::LooseEq,
                Token::BangEq => BinOp::LooseNeq,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.parse_comparison()?;
            left = self.ast.add_expr(Expr::BinOp { op, left, right });
        }
    }

    pub(super) fn parse_comparison(&mut self) -> Result<ExprId, String> {
        // §13.10 `PrivateIdentifier in ShiftExpression` — its own
        // grammar arm, split to sibling `private_in.rs` (doc there).
        if let Some(e) = self.try_parse_private_in()? {
            return Ok(e);
        }
        let mut left = self.parse_shift()?;
        loop {
            // `expr instanceof rhs` (ES §13.10.2) — relational-precedence
            // operator whose right-hand side is a general expression, so
            // it parses at shift level exactly like the `in` operator
            // below.
            if matches!(self.peek(), Token::InstanceOf) {
                self.pos += 1;
                let rhs = self.parse_shift()?;
                // `x instanceof F` where F binds a class expression
                // (`const F = class ... {}`) — resolve through the
                // parse-order alias map to the synth class name
                // (`__ClassExpr_<id>`), same consumption as `new F()`
                // (primary_new_super) and `F.method()` (parse_postfix).
                // The lowering's descendant-tag set is keyed on real
                // class names; the raw binding name would miss and
                // constant-fold to false.
                //
                // Only a BARE name can be aliased: the map is keyed on
                // binding names, and anything larger (`box.cls`,
                // `(C as any)`, a call) is a value the runtime operator
                // resolves for itself.
                let rhs = match self.ast.get_expr(rhs) {
                    Expr::Ident(n) => match self.class_value_aliases.get(n) {
                        Some(a) => {
                            let a = a.clone();
                            self.ast.add_expr(Expr::Ident(a))
                        }
                        None => rhs,
                    },
                    _ => rhs,
                };
                left = self.ast.add_expr(Expr::InstanceOf { expr: left, rhs });
                continue;
            }
            // T-45 — binary `in` operator. JS contextual keyword:
            // `<key> in <obj>` returns true if obj has property key.
            // tora's lexer keeps "in" as Token::Ident("in") so the
            // for-in loop parser can detect it; here we accept it as
            // a binary operator at relational precedence and emit a
            // synthetic Call to `__torajs_in_op(key, obj)` (which
            // check.rs/ssa_lower intercept by name) — avoids adding
            // a new Expr variant that every recursive walker would
            // need to handle exhaustively.
            if matches!(self.peek(), Token::Ident(n) if n == "in") {
                // §14.7.4 / §13.13 — the RelationalExpression[~In]
                // production has no `in` arm, so under the parameter
                // the expression simply ENDS here and the token is
                // left for whoever asked. That is what `[~In]` means
                // grammatically, and the two askers both want it:
                // a C-style for-head then expects `;`, finds `in`
                // and refuses (`for (true ? 0 : 0 in {};;)` —
                // test262 conditional/in-branch-2), while Annex B
                // §B.3.5's `for (var x = 5 in o)` head expects
                // exactly this token and goes on. Reporting the
                // error from HERE could only serve the first.
                // Parentheses and a ternary's THEN branch reset the
                // restriction; the for-in statement never reaches
                // here (try_parse_for_of claims it first).
                if self.in_for_init {
                    return Ok(left);
                }
                self.pos += 1;
                let right = self.parse_shift()?;
                let callee = self.ast.add_expr(Expr::Ident("__torajs_in_op".to_string()));
                left = self.ast.add_expr(Expr::Call {
                    callee,
                    args: vec![left, right],
                });
                continue;
            }
            let op = match self.peek() {
                Token::Lt => BinOp::Lt,
                Token::Gt => BinOp::Gt,
                Token::LtEq => BinOp::Le,
                Token::GtEq => BinOp::Ge,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.parse_shift()?;
            left = self.ast.add_expr(Expr::BinOp { op, left, right });
        }
    }

    pub(super) fn parse_shift(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_additive()?;
        loop {
            // V3-18 wedge — `<<=` `>>=` `>>>=` belong to parse_assign.
            if matches!(
                self.tokens.get(self.pos + 1).map(|s| &s.token),
                Some(Token::Eq)
            ) && matches!(
                self.peek(),
                Token::ShlShl | Token::ShrShr | Token::ShrShrShr
            ) {
                return Ok(left);
            }
            let op = match self.peek() {
                Token::ShlShl => BinOp::Shl,
                Token::ShrShr => BinOp::Shr,
                Token::ShrShrShr => BinOp::UShr,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.parse_additive()?;
            left = self.ast.add_expr(Expr::BinOp { op, left, right });
        }
    }

    pub(super) fn parse_additive(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.parse_multiplicative()?;
            left = self.ast.add_expr(Expr::BinOp { op, left, right });
        }
    }

    pub(super) fn parse_multiplicative(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_pow()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.parse_pow()?;
            left = self.ast.add_expr(Expr::BinOp { op, left, right });
        }
    }

    /* V3-01 — `**` exponent. JS spec: precedence above mul/div/mod
     * (which is why parse_multiplicative now defers to this), and
     * **right-associative** (`2 ** 3 ** 2` = `2 ** (3 ** 2)` =
     * `2 ** 9` = 512). */
    pub(super) fn parse_pow(&mut self) -> Result<ExprId, String> {
        let start = self.pos;
        let left = self.parse_unary()?;
        if matches!(self.peek(), Token::StarStar) {
            // §13.6 — the left side of `**` is an UpdateExpression,
            // not a UnaryExpression, so `-a ** b` is a SyntaxError and
            // `(-a) ** b` is how you say it. The AST records no
            // parentheses, so the question has to be asked of the
            // TOKEN this operand started at.
            //
            // `--a ** b` is legal: `++` / `--` ARE UpdateExpression,
            // which is exactly the production the rule admits.
            //
            // Until now `-a ** b` parsed as `-(a ** b)` and answered
            // -8. Three test262 cases still reported pass-negative for
            // it, because the checker happened to refuse the operand
            // pair on TYPE grounds and the runner read that refusal as
            // the expected early error. Widening `**` to its siblings'
            // coercibles took that accident away and showed what had
            // been standing in for the rule.
            if matches!(
                self.tokens.get(start).map(|s| &s.token),
                Some(
                    Token::Bang
                        | Token::Minus
                        | Token::Plus
                        | Token::Tilde
                        | Token::TypeOf
                        | Token::Delete
                        | Token::Void
                        | Token::Await
                )
            ) {
                return Err(format!(
                    "unary operand of `**` must be parenthesized (ES §13.6) at {}",
                    self.at()
                ));
            }
            self.pos += 1;
            let right = self.parse_pow()?;
            return Ok(self.ast.add_expr(Expr::BinOp {
                op: BinOp::Pow,
                left,
                right,
            }));
        }
        Ok(left)
    }

    pub(super) fn parse_unary(&mut self) -> Result<ExprId, String> {
        if matches!(self.peek(), Token::Lt) {
            return self.parse_angle_assertion();
        }
        if matches!(self.peek(), Token::Bang) {
            self.pos += 1;
            // Right-associative: `!!a` = !(!a).
            let inner = self.parse_unary()?;
            return Ok(self.ast.add_expr(Expr::Unary {
                op: ast::UnaryOp::Not,
                expr: inner,
            }));
        }
        if matches!(self.peek(), Token::Minus) {
            self.pos += 1;
            let inner = self.parse_unary()?;
            return Ok(self.ast.add_expr(Expr::Unary {
                op: ast::UnaryOp::Neg,
                expr: inner,
            }));
        }
        // V3-18 m1.h.4 — unary `+x` ToNumber.
        if matches!(self.peek(), Token::Plus) {
            self.pos += 1;
            let inner = self.parse_unary()?;
            return Ok(self.ast.add_expr(Expr::Unary {
                op: ast::UnaryOp::Plus,
                expr: inner,
            }));
        }
        if matches!(self.peek(), Token::Tilde) {
            self.pos += 1;
            let inner = self.parse_unary()?;
            return Ok(self.ast.add_expr(Expr::Unary {
                op: ast::UnaryOp::BitNot,
                expr: inner,
            }));
        }
        if matches!(self.peek(), Token::TypeOf) {
            self.pos += 1;
            let inner = self.parse_unary()?;
            return Ok(self.ast.add_expr(Expr::TypeOf { expr: inner }));
        }
        // `delete <expr>` — the operand triage lives in
        // `parser/delete_expr.rs` (§13.5.1: real delete vs strict
        // early error vs non-reference true-lane).
        if matches!(self.peek(), Token::Delete) {
            self.pos += 1;
            return self.parse_delete_operand();
        }
        if matches!(self.peek(), Token::Void) {
            return self.parse_void_expr();
        }
        // L.2 — `await <expr>` extracts the resolved value from a
        // Promise. MVP desugar: `await e` ⇒ `e.value` (synchronous
        // read, well-defined only for already-fulfilled promises in
        // the L.1 eager-fire model). Right-associative so chained
        // forms parse like other unary prefixes.
        if matches!(self.peek(), Token::Await) {
            // §13.1.1 — the word reads as an IdentifierReference where
            // no AwaitExpression can stand, and at a program's top
            // level where the next token settles the tie. The
            // module-goal half is the recorded site's job
            // (`ast::triage_await_idents`), the goal bit being stamped
            // after the parse. Checked BEFORE the two rejects below:
            // both of them are about an AwaitExpression, and this is
            // the branch that says there isn't one.
            if self.await_reads_as_ref() {
                return self.parse_postfix();
            }
            if self.in_formal_params {
                return Err(format!(
                    "`await` may not be used in a formal parameter list at {} (ES §15.8.1)",
                    self.at()
                ));
            }
            if !self.await_allowed {
                // §13.1.1 — no AwaitExpression may stand here, so under
                // the SCRIPT goal the word is an ordinary
                // IdentifierReference (test262 `await-in-function.js`,
                // `await-in-generator.js`). There is no ambiguity to
                // resolve: a position that cannot hold an
                // AwaitExpression leaves the name reading as the only
                // one. `await_reserved` is what still refuses it —
                // inside an async body the two bits disagree only at a
                // program's top level, and inside a static block the
                // grammar parameter is on.
                return Err(format!(
                    "`await` is only valid in async functions and at the top level of a \
                     module at {} (ES §15.8.1)",
                    self.at()
                ));
            }
            let script_may_parse = self.await_operand_may_continue_name();
            self.note_module_only_await(script_may_parse);
            self.pos += 1;
            let inner = self.parse_unary()?;
            let read = self.ast.add_expr(Expr::Member {
                obj: inner,
                name: "value".into(),
            });
            // rotation 233 — mark the minted read so the checker /
            // lowering dispatch it by TYPE (§27.7.5.1: Promise
            // unwraps, everything else passes through identity)
            // instead of falling into the field lookup a user's
            // `{value: T}` struct would win.
            self.ast.await_value_reads.insert(read);
            return Ok(read);
        }
        // Pre-increment / pre-decrement — the same UpdateExpression
        // node as the postfix form, answering the new value. It used
        // to desugar to `x = x + 1`, which is `+` rather than
        // ToNumeric: `++x` on an `any` holding `"1"` concatenated to
        // `"11"` (§13.4.6 says 2).
        if matches!(self.peek(), Token::PlusPlus | Token::MinusMinus) {
            let is_inc = matches!(self.peek(), Token::PlusPlus);
            self.pos += 1;
            let target = self.parse_unary()?;
            self.reject_invalid_assignment_target(target)?;
            self.reject_invalid_update_target(target)?;
            return Ok(self.ast.add_expr(Expr::PostIncr {
                target,
                is_inc,
                is_prefix: true,
            }));
        }
        self.parse_postfix()
    }
}
