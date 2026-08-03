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
        let then_branch = self.with_yield_hoist_disallowed(|p| p.parse_assign())?;
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
        // V3-18 wedge — `??=` (logical-nullish assign) must be left
        // for parse_assign to handle. Decline `??` here when `=`
        // follows.
        while matches!(self.peek(), Token::QuestionQuestion)
            && !matches!(
                self.tokens.get(self.pos + 1).map(|s| &s.token),
                Some(Token::Eq)
            )
        {
            self.pos += 1;
            // `??` rhs is conditionally evaluated — no yield hoist.
            let right = self.with_yield_hoist_disallowed(|p| p.parse_logical_or())?;
            left = self.ast.add_expr(Expr::Nullish {
                lhs: left,
                rhs: right,
            });
        }
        Ok(left)
    }

    pub(super) fn parse_logical_or(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_logical_and()?;
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
        }
        Ok(left)
    }

    pub(super) fn parse_logical_and(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_bit_or()?;
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
        }
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
        let mut left = self.parse_shift()?;
        loop {
            // `expr instanceof ClassName` — relational-precedence operator.
            // Right-hand side is a single bare identifier (the class name),
            // not a general expression — tr resolves the class statically.
            if matches!(self.peek(), Token::InstanceOf) {
                self.pos += 1;
                let class_name = match self.peek() {
                    Token::Ident(s) => s.clone(),
                    other => {
                        return Err(format!(
                            "expected class name after `instanceof`, got {other:?}"
                        ));
                    }
                };
                self.pos += 1;
                // `x instanceof F` where F binds a class expression
                // (`const F = class ... {}`) — resolve through the
                // parse-order alias map to the synth class name
                // (`__ClassExpr_<id>`), same consumption as `new F()`
                // (primary_new_super) and `F.method()` (parse_postfix).
                // The lowering's descendant-tag set is keyed on real
                // class names; the raw binding name would miss and
                // constant-fold to false.
                let class_name = self
                    .class_value_aliases
                    .get(&class_name)
                    .cloned()
                    .unwrap_or(class_name);
                left = self.ast.add_expr(Expr::InstanceOf {
                    expr: left,
                    class_name,
                });
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
        // `delete obj.k` per ES §13.5.1 — only a property reference
        // is accepted: deleting a variable is the strict-mode
        // SyntaxError (modules are strict), and the exotic
        // non-reference forms (`delete 42`, always true) stay a loud
        // recorded boundary.
        if matches!(self.peek(), Token::Delete) {
            self.pos += 1;
            let inner = self.parse_unary()?;
            // §13.5.1.1 early error — the operand must not be a
            // private reference (`delete o.#m`), however the
            // receiver is shaped. Parens are transparent in the
            // arena, so the covered forms land here too. A private
            // name parses to its `__priv_<cls>__<n>` mangling
            // (member_name_after_dot), so that prefix is the marker.
            if let Expr::Member { name, .. } = self.ast.get_expr(inner)
                && name.starts_with("__priv_")
            {
                let bare = name.rsplit("__").next().unwrap_or(name);
                return Err(format!(
                    "deleting the private name `#{bare}` is forbidden at {}",
                    self.at()
                ));
            }
            if !matches!(
                self.ast.get_expr(inner),
                Expr::Member { .. } | Expr::Index { .. }
            ) {
                return Err(format!(
                    "`delete` target must be a property reference (obj.k / obj[k]) at {}",
                    self.at()
                ));
            }
            return Ok(self.ast.add_expr(Expr::Delete { expr: inner }));
        }
        // V3-18 m1.h.30 — `void <expr>` evaluates expr (for side
        // effects) then yields `undefined`. Desugars to
        // `Expr::Sequence { left: <expr>, right: Expr::Ident
        // ("undefined") }` so `void 0` is the same value as the
        // `undefined` Ident everywhere: Type::Undefined at check
        // time (binop undef-id hints fire), ConstPtrNull at SSA.
        // RC-4 F1b-1: the earlier String("undefined") stand-in
        // made `x !== void 0` a *content* compare (str_eq) — a
        // real "undefined" string compared equal to the undefined
        // literal, and a null-slot Str operand SIGSEGV'd inside
        // str_eq (test262 S15.5.4.10 family).
        if matches!(self.peek(), Token::Void) {
            self.pos += 1;
            let inner = self.parse_unary()?;
            // RFC 20260713-array-proto-residual blade 5 — a pure
            // literal operand folds to the plain `undefined` ident
            // (ES §13.5.2 evaluates then discards; literals have no
            // effects). The Sequence wrapper defeated every
            // undefined-shape probe downstream (any-literal pack /
            // let-binding lanes tagged `void 0` as null — printed
            // "null", typeof "object"). Effectful operands keep the
            // Sequence (evaluation order preserved).
            if matches!(
                self.ast.get_expr(inner),
                Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null
            ) || matches!(self.ast.get_expr(inner), Expr::Ident(n) if n == "undefined")
            {
                return Ok(self.ast.add_expr(Expr::Ident("undefined".into())));
            }
            let undef = self.ast.add_expr(Expr::Ident("undefined".into()));
            return Ok(self.ast.add_expr(Expr::Sequence {
                left: inner,
                right: undef,
            }));
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
