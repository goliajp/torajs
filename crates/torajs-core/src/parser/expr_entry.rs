//! Expression-entry cluster (chunk 412).
//!
//! Extracted verbatim from parser.rs — 4 methods forming the top of
//! the expression grammar, plus one AST-cloning helper used by the
//! compound-assign desugar:
//! - lower_template_parts — template-literal stitcher
//!   (`\`hello ${expr} world\`` → String + Expr + String chain)
//! - parse_expr — trivial `expr := assign` alias
//! - parse_assign — right-associative `=` / compound-assign
//!   (`+=` / `-=` / `*=` / `/=` / `%=` / `**=` / `<<=` / `>>=` /
//!   `>>>=` / `&=` / `|=` / `^=` / `&&=` / `||=` / `??=`)
//! - clone_expr_for_compound — fresh top-level Ident/Member/Index
//!   node so compound assigns can name the LHS twice
//!
//! parse_assign + clone_expr_for_compound were already `pub(super)`
//! (chunk 408 promoted them for expr_prec.rs to call parse_ternary);
//! lower_template_parts + parse_expr promoted here too. Body unchanged.

use super::*;

impl<'a> Parser<'a> {
    /// Stitch a `Token::Template`'s parts into an `Expr::String + Expr +
    /// Expr::String + …` chain. Single-part literal templates collapse
    /// to a bare `Expr::String`. Empty-parts templates collapse to
    /// `Expr::String("")`. Each interpolation gets a sub-Parser so the
    /// recursive lex output can be consumed without re-tokenizing.
    ///
    /// Performance: chain of `+`s reuses the existing string-concat fast
    /// path, including number→string auto-coercion. For N interpolations
    /// with K number values: K num→str allocs + N concats. The parser
    /// could in principle build an Array+join (1 array alloc + 1 join
    /// alloc instead of N concats) for ≥3 interpolations; deferred to a
    /// later optimization once profiling shows template heavy use.
    pub(super) fn lower_template_parts(
        &mut self,
        parts: &[lexer::TemplatePart],
    ) -> Result<ExprId, String> {
        if parts.is_empty() {
            return Ok(self.ast.add_expr(Expr::String(String::new())));
        }
        // Special-case all-literal templates → emit a single
        // Expr::String. Common case `\`hello\`` skips the chain entirely.
        if parts.len() == 1 {
            if let lexer::TemplatePart::Lit(s) = &parts[0] {
                return Ok(self.ast.add_expr(Expr::String(s.clone())));
            }
        }
        let mut acc: Option<ExprId> = None;
        for p in parts {
            let id = match p {
                lexer::TemplatePart::Lit(s) => {
                    if s.is_empty() && acc.is_some() {
                        // Skip empty-string filler between adjacent
                        // interpolations — `${a}${b}` shouldn't
                        // generate an extra `+ ""` step.
                        continue;
                    }
                    self.ast.add_expr(Expr::String(s.clone()))
                }
                lexer::TemplatePart::Expr(tokens) => {
                    let mut sub = Parser {
                        // Template interpolation tokens were extracted
                        // from `self.source` — the same slice satisfies
                        // ASI's byte-span probe for the sub-parse.
                        source: self.source,
                        tokens,
                        pos: 0,
                        type_close_peel: 0,
                        type_ann_depth: 0,
                        ast: std::mem::take(&mut self.ast),
                        desugar_id: self.desugar_id,
                        generator_fns: std::mem::take(&mut self.generator_fns),
                        current_class: self.current_class.clone(),
                        // Inherited for the same reason `current_class`
                        // is: `${this.x}` inside a class generator
                        // method's body must mint the same receiver
                        // reference the surrounding body does.
                        in_gen_class_method: self.in_gen_class_method,
                        // Same inheritance rationale: `${this.x}` in a
                        // static method body must mint the class-object
                        // reference the surrounding body does (S2.37).
                        static_this_class: self.static_this_class.clone(),
                        // Likewise: `${super.m()}` in a derived ctor is
                        // as legal as the same call written outside the
                        // template, so the sub-parse inherits the
                        // position rather than resetting it.
                        super_call_allowed: self.super_call_allowed,
                        // A template interpolation cannot contain a
                        // statement-level `yield*`, but the flag rides
                        // along like the other position markers.
                        in_async_gen: self.in_async_gen,
                        pending_async_fn_expr: false,
                        current_class_has_parent: self.current_class_has_parent,
                        synth_classes: Vec::new(),
                        // Sub-parser sees outer aliases so a template
                        // interpolation can do `${new F()}` where F is
                        // an outer const-class binding. Sub-parser
                        // never adds aliases itself (only stmt-level
                        // const-decls register).
                        class_value_aliases: self.class_value_aliases.clone(),
                        dyn_import_counter: self.dyn_import_counter,
                        // `${yield x}` is legal in a generator —
                        // hoisted YieldIntos flow back to the outer
                        // buffer below, position marker rides along.
                        yield_hoist_buf: Vec::new(),
                        yield_hoist_allowed: self.yield_hoist_allowed,
                    };
                    let result = sub.parse_expr()?;
                    // Tokens vec ends with Token::Eof; anything before
                    // Eof past the parsed expr is leftover input.
                    if !matches!(sub.peek(), Token::Eof) {
                        return Err(format!(
                            "unexpected trailing tokens in template interpolation: {:?}",
                            sub.peek()
                        ));
                    }
                    self.ast = sub.ast;
                    self.desugar_id = sub.desugar_id;
                    self.generator_fns = sub.generator_fns;
                    // P8.5 — propagate any class expressions parsed
                    // inside the template interpolation back to the
                    // outer parser so they flush at the enclosing
                    // stmt boundary.
                    self.synth_classes.append(&mut sub.synth_classes);
                    // Expression-position yields hoisted inside the
                    // interpolation drain at the ENCLOSING statement.
                    self.yield_hoist_buf.append(&mut sub.yield_hoist_buf);
                    // P13-S5 — propagate dynamic-import counter so
                    // the next minted name doesn't collide.
                    self.dyn_import_counter = sub.dyn_import_counter;
                    // §13.2.8.5 — a substitution stringifies with the
                    // STRING hint (ToString → toString before
                    // valueOf). The desugared `+` chain can't carry
                    // that: `+`'s object operands run
                    // ToPrimitive(default) — valueOf first — so
                    // `${objWithBothHooks}` observably diverged from
                    // bun once the concat lanes took the spec order.
                    // Wrapping in `String(...)` makes the hint
                    // explicit; the String() lowering is a typed
                    // dispatch (identity on Str, the same
                    // *_to_str intrinsics elsewhere), not a real
                    // call, so primitive substitutions cost nothing.
                    let callee = self.ast.add_expr(Expr::Ident("String".to_string()));
                    // §13.2.8.6 — a substitution runs the IMPLICIT
                    // ToString (a Symbol throws TypeError), while the
                    // `String(...)` spelling below is the one explicit
                    // lane that stringifies a Symbol. Key the
                    // synthesized callee so the lowering can keep both
                    // faces apart.
                    self.ast.template_str_calls.insert(callee);
                    self.ast.add_expr(Expr::Call {
                        callee,
                        args: vec![result],
                    })
                }
            };
            acc = Some(match acc {
                None => id,
                Some(prev) => self.ast.add_expr(Expr::BinOp {
                    op: BinOp::Add,
                    left: prev,
                    right: id,
                }),
            });
        }
        // If acc is still None (everything was empty Lit), produce "".
        Ok(acc.unwrap_or_else(|| self.ast.add_expr(Expr::String(String::new()))))
    }

    pub(super) fn parse_expr(&mut self) -> Result<ExprId, String> {
        self.parse_assign()
    }

    /// RC-3 (RFC 20260706-test262-bug-corpus) — any reassignment of a
    /// class-value alias binding drops the P8.5 static alias; later
    /// `C.m()` / `new C()` fall back to the dynamic path instead of
    /// silently binding the old class.
    fn drop_class_alias_on_assign(&mut self, target: ExprId) {
        if let Expr::Ident(n) = self.ast.get_expr(target)
            && self.class_value_aliases.contains_key(n)
        {
            let n = n.clone();
            self.class_value_aliases.remove(&n);
        }
    }

    pub(super) fn parse_assign(&mut self) -> Result<ExprId, String> {
        // §15.5.5 — YieldExpression is an AssignmentExpression
        // alternative; expression-position yields hoist to a
        // `YieldInto` temp (yield_expr_hoist.rs). The statement /
        // let-init lanes peeked their `yield` earlier, so this only
        // sees genuinely nested positions.
        if matches!(self.peek(), Token::Yield) {
            return self.parse_yield_expr_hoist();
        }
        let target = self.parse_ternary()?;
        // V3-18 wedge — ES2021 logical assignment: `??=` / `||=` /
        // `&&=`. Detected here (after the lhs is parsed) by peeking
        // a two-token sequence; parse_nullish / parse_logical_or /
        // parse_logical_and decline to consume their op when an `=`
        // follows so this branch sees them.
        let logical_assign: Option<&str> =
            match (self.peek(), self.tokens.get(self.pos + 1).map(|s| &s.token)) {
                (Token::QuestionQuestion, Some(Token::Eq)) => Some("??"),
                (Token::PipePipe, Some(Token::Eq)) => Some("||"),
                (Token::AmpAmp, Some(Token::Eq)) => Some("&&"),
                _ => None,
            };
        if let Some(op_name) = logical_assign {
            self.pos += 2;
            self.reject_yield_temp_target(target)?;
            self.drop_class_alias_on_assign(target);
            // `??= ||= &&=` rhs only evaluates when the guard fires —
            // conditional position, no yield hoist.
            let value = self.with_yield_hoist_disallowed(|p| p.parse_assign())?;
            let lhs = self.clone_expr_for_compound(target);
            // ES2021 §13.15 requires short-circuit — PutValue must not
            // fire when the lhs already satisfies the guard (truthy for
            // ||=, falsy for &&=, non-nullish for ??=). Desugar so the
            // Assign sits INSIDE the branch that runs only on the
            // "assign needed" path:
            //   x ??= y  →  x ?? (x = y)
            //   x ||= y  →  x || (x = y)
            //   x &&= y  →  x && (x = y)
            // (Previously `x = (x op y)` unconditionally called
            // PutValue, which throws on non-writeable / non-extensible
            // targets even when the assign is spec-optional.)
            let assign = self.ast.add_expr(Expr::Assign { target, value });
            let expr = match op_name {
                "??" => Expr::Nullish { lhs, rhs: assign },
                "||" => Expr::BinOp {
                    op: BinOp::LOr,
                    left: lhs,
                    right: assign,
                },
                "&&" => Expr::BinOp {
                    op: BinOp::LAnd,
                    left: lhs,
                    right: assign,
                },
                _ => unreachable!(),
            };
            return Ok(self.ast.add_expr(expr));
        }
        // V3-18 wedge — bitwise compound assignments (`|= ^= &= <<= >>=
        // >>>=`) per JS spec §13.15. Same desugar shape as the other
        // compound forms — `target = target <op> value` with a cloned
        // lhs read. Lex emits these as 2- / 3-token sequences (e.g.
        // `Pipe Eq`, `ShrShr Eq`).
        let bit_assign: Option<BinOp> =
            match (self.peek(), self.tokens.get(self.pos + 1).map(|s| &s.token)) {
                (Token::Pipe, Some(Token::Eq)) => Some(BinOp::BitOr),
                (Token::Caret, Some(Token::Eq)) => Some(BinOp::BitXor),
                (Token::Amp, Some(Token::Eq)) => Some(BinOp::BitAnd),
                (Token::ShlShl, Some(Token::Eq)) => Some(BinOp::Shl),
                (Token::ShrShr, Some(Token::Eq)) => Some(BinOp::Shr),
                (Token::ShrShrShr, Some(Token::Eq)) => Some(BinOp::UShr),
                _ => None,
            };
        if let Some(op) = bit_assign {
            self.pos += 2;
            self.reject_yield_temp_target(target)?;
            self.drop_class_alias_on_assign(target);
            let value = self.parse_assign()?;
            let lhs = self.clone_expr_for_compound(target);
            let rhs = self.ast.add_expr(Expr::BinOp {
                op,
                left: lhs,
                right: value,
            });
            return Ok(self.ast.add_expr(Expr::Assign { target, value: rhs }));
        }
        // Plain `=` and the compound forms (`+= -= *= /= %=`). Compound
        // forms desugar at the parser level into `target = target op value`,
        // matching JS shape without needing a new AST variant.
        let compound_op = match self.peek() {
            Token::Eq => None,
            Token::PlusEq => Some(BinOp::Add),
            Token::MinusEq => Some(BinOp::Sub),
            Token::StarEq => Some(BinOp::Mul),
            Token::StarStarEq => Some(BinOp::Pow),
            Token::SlashEq => Some(BinOp::Div),
            Token::PercentEq => Some(BinOp::Mod),
            _ => return Ok(target),
        };
        self.pos += 1;
        self.reject_yield_temp_target(target)?;
        self.drop_class_alias_on_assign(target);
        let value = self.parse_assign()?; // right-associative
        if let Some(op) = compound_op {
            // For idents we re-use the same target ExprId on the rhs;
            // for member/index targets we must clone the access (since
            // they're side-effect-free in our subset). Easiest: clone
            // the AST node by re-evaluating at the rhs position. tr's
            // AST is arena-backed so we just append a fresh expr that
            // re-reads the same Ident / Member / Index.
            let lhs = self.clone_expr_for_compound(target);
            let rhs = self.ast.add_expr(Expr::BinOp {
                op,
                left: lhs,
                right: value,
            });
            return Ok(self.ast.add_expr(Expr::Assign { target, value: rhs }));
        }
        Ok(self.ast.add_expr(Expr::Assign { target, value }))
    }

    /// Compound assign desugar helper: produce a fresh `ExprId` that
    /// references the same identifier/member/index as `eid`. We can
    /// share scalar/binop sub-trees, but the LHS appears twice in the
    /// desugared `x = x + v`, and treating it as a literal share would
    /// confuse downstream passes that index by ExprId. So make a
    /// fresh top-level node — for the shapes the parser produces here
    /// (`Ident`, `Member`, `Index`) the read is side-effect-free.
    pub(super) fn clone_expr_for_compound(&mut self, eid: ExprId) -> ExprId {
        let cloned = match self.ast.get_expr(eid) {
            Expr::Ident(name) => Expr::Ident(name.clone()),
            Expr::Member { obj, name } => Expr::Member {
                obj: *obj,
                name: name.clone(),
            },
            Expr::Index { obj, index } => Expr::Index {
                obj: *obj,
                index: *index,
            },
            other => panic!("parser: invalid compound-assign target shape {other:?}"),
        };
        self.ast.add_expr(cloned)
    }
}
