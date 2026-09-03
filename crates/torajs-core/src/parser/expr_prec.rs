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
        let saved_in_for_init = std::mem::replace(&mut self.in_for_init, false);
        let then_branch = self.with_yield_hoist_disallowed(|p| p.parse_assign())?;
        self.in_for_init = saved_in_for_init;
        if !matches!(self.peek(), Token::Colon) {
            return Err(format!(
                "expected `:` in ternary expression, got {:?}",
                self.peek()
            ));
        }
        self.pos += 1;
        let else_branch = self.with_yield_hoist_disallowed(|p| p.parse_assign())?;
        Ok(self.ast.add_expr(Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        }))
    }

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
                // production has no `in` arm: a C-style for-head
                // init refuses it at PARSE time (`for (true ? 0 :
                // 0 in {};;)` — test262 conditional/in-branch-2,
                // which the ternary type reject used to catch by
                // coincidence, in the wrong phase). Parentheses and
                // a ternary's THEN branch reset the restriction;
                // the for-in statement never reaches here
                // (try_parse_for_of claims it first).
                if self.in_for_init {
                    return Err(format!(
                        "`in` is not allowed in a for-statement head (at {})",
                        self.at()
                    ));
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
     * `2 ** 9` = 512). Spec also requires parens around any unary
     * operand of `**` (e.g. `-2 ** 2` is a SyntaxError per spec);
     * we accept it as `-(2 ** 2)` for now and ship a stricter
     * check alongside the test262 push (V3-18 in the v3 plan). */
    pub(super) fn parse_pow(&mut self) -> Result<ExprId, String> {
        let left = self.parse_unary()?;
        if matches!(self.peek(), Token::StarStar) {
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
            if self.in_formal_params {
                return Err(format!(
                    "`await` may not be used in a formal parameter list at {} (ES §15.8.1)",
                    self.at()
                ));
            }
            if !self.await_allowed {
                return Err(format!(
                    "`await` is only valid in async functions and at the top level of a \
                     module at {} (ES §15.8.1)",
                    self.at()
                ));
            }
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
        // Pre-increment / pre-decrement: `++x` desugars to `x = x + 1`,
        // value is the new x. We emit an Assign whose target is the
        // ident binding; the result of an Assign expression in the
        // existing AST already evaluates to the new value.
        if matches!(self.peek(), Token::PlusPlus | Token::MinusMinus) {
            let is_inc = matches!(self.peek(), Token::PlusPlus);
            self.pos += 1;
            let target = self.parse_unary()?;
            self.reject_invalid_assignment_target(target)?;
            let lhs_clone = self.clone_expr_for_compound(target);
            let one = self.ast.add_expr(Expr::Number(1.0));
            let op = if is_inc { BinOp::Add } else { BinOp::Sub };
            let rhs = self.ast.add_expr(Expr::BinOp {
                op,
                left: lhs_clone,
                right: one,
            });
            return Ok(self.ast.add_expr(Expr::Assign { target, value: rhs }));
        }
        self.parse_postfix()
    }
}
