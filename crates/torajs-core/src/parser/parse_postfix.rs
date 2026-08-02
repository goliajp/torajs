//! `Parser::parse_postfix` extracted from `parser.rs` (chunk 164).
//!
//! `parse_postfix` handles JS postfix operators applied to a
//! primary expression: member access (`.`), index access (`[`),
//! call (`(`), postfix `++` / `--`, optional chaining (`?.`),
//! template tagged calls, and non-null assertion (`!`). The loop
//! dispatches per token; fat arms live in per-arm helpers below.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_postfix(&mut self) -> Result<ExprId, String> {
        let start_pos = self.pos;
        let mut node = self.parse_primary()?;
        loop {
            match self.peek() {
                Token::Dot => {
                    self.pos += 1;
                    let name = self.expect_member_name(".")?;
                    // RC-3 (RFC 20260706-test262-bug-corpus) — member
                    // access on a class-expression alias binding
                    // (`const/var C = class {…}; C.method(…)`) swaps
                    // the receiver to the synth class Ident so the
                    // named-class static machinery applies unchanged.
                    // Same linear-parse-order contract as the P8.5
                    // `new F()` rewrite in parse_new.
                    if let Expr::Ident(n) = self.ast.get_expr(node)
                        && let Some(target) = self.class_value_aliases.get(n)
                    {
                        let target = target.clone();
                        node = self.add_expr_at(start_pos, Expr::Ident(target));
                    }
                    node = self.add_expr_at(start_pos, Expr::Member { obj: node, name });
                }
                Token::QuestionDot => {
                    self.pos += 1;
                    if matches!(self.peek(), Token::LBracket) {
                        node = self.parse_optchain_index(node, start_pos)?;
                    } else if matches!(self.peek(), Token::LParen) {
                        // `callee?.(args…)` — ES2020 optional call
                        // (chunk 705). Args parse eagerly; lowering
                        // guards their evaluation behind the nullish
                        // short-circuit — so they are a conditional
                        // position: no yield hoist.
                        self.pos += 1;
                        let args = self.with_yield_hoist_disallowed(|p| p.parse_call_args())?;
                        node = self.add_expr_at(start_pos, Expr::OptCall { callee: node, args });
                    } else {
                        let name = self.expect_member_name("?.")?;
                        node = self.add_expr_at(start_pos, Expr::OptChain { obj: node, name });
                    }
                }
                Token::LParen => {
                    self.pos += 1;
                    let args = self.parse_call_args()?;
                    node = self.add_expr_at(start_pos, Expr::Call { callee: node, args });
                }
                Token::LBracket => {
                    node = self.parse_postfix_index(node, start_pos)?;
                }
                Token::PlusPlus | Token::MinusMinus => {
                    // Post-increment / post-decrement: `x++` / `x--`.
                    // JS spec: yields the OLD value, then mutates. ssa_lower
                    // handles the temp-and-store-back machinery directly via
                    // `Expr::PostIncr`. (Pre-increment uses `Expr::Assign`
                    // with a `target = target + 1` shape — that's already
                    // covered by the prefix-side parser.)
                    //
                    // ES §12.5.8 / §12.5.9 restricted production:
                    // `LHS [no LineTerminator here] (++|--)` — a newline
                    // between the LHS and `++` / `--` breaks the postfix,
                    // and the operator becomes a prefix on the next stmt.
                    // Pre-fix `x\n++y` parsed as `x++; y;` (assigning back
                    // to `x`) instead of `x;\n++y;` (leaving x untouched
                    // and pre-incrementing y).
                    if self.has_newline_before(self.pos) {
                        return Ok(node);
                    }
                    let is_inc = matches!(self.peek(), Token::PlusPlus);
                    self.pos += 1;
                    node = self.ast.add_expr(Expr::PostIncr {
                        target: node,
                        is_inc,
                    });
                }
                Token::Template { .. } => {
                    // T-12 (v0.4.0) — tagged template literal
                    // `tag`...${expr}...``. Requires a separate
                    // substrate item: parser support for the call
                    // shape, AST node carrying both raw + cooked
                    // strings arrays, runtime emission of the raw
                    // array, and `String.raw` dispatch on top. The
                    // generic parse error ("expected `)`") would be
                    // confusing — emit a clear deferral pointer
                    // instead. Lands post-v0.4.0 alongside a full
                    // tagged-template substrate item.
                    return Err(format!(
                        "tagged template literal `tag\\`...\\`` not yet supported \
                         (planned post-v0.4.0 substrate item; see docs/roadmap.md \
                         T-12 followup) at {}",
                        self.at()
                    ));
                }
                // V3-07 — `expr as T` TS type cast as a postfix
                // shape. Binding here is tighter than any binary op
                // (so `arr.push(self as any)` parses without ambiguity);
                // wider TS forms like `(a + b) as number` work via
                // the explicit paren grouping.
                Token::Ident(s) if s == "as" => {
                    self.pos += 1;
                    // V3-18 wedge — `<expr> as const` (TS const
                    // assertion) is no-op at runtime; subset treats
                    // it as identity. `<expr> satisfies T` likewise
                    // (TS-only type-check assist).
                    if matches!(self.peek(), Token::Const) {
                        self.pos += 1;
                        // No type to record; identity cast.
                        continue;
                    }
                    let ty_ann = self.parse_type_ann()?;
                    node = self.add_expr_at(start_pos, Expr::As { expr: node, ty_ann });
                }
                // V3-18 wedge — TS non-null assertion `<expr>!`. Pure
                // type-side; runtime no-op. Detect only when the `!`
                // is followed by something that would be valid after
                // a postfix (not the start of another expression like
                // `!x` prefix).
                Token::Bang => {
                    if !self.bang_is_postfix() {
                        return Ok(node);
                    }
                    self.pos += 1;
                    // Encode as `As { ty_ann: "__nonnull__" }` so
                    // check.rs can narrow Nullable<T> → T while
                    // ssa_lower keeps it as identity.
                    node = self.add_expr_at(
                        start_pos,
                        Expr::As {
                            expr: node,
                            ty_ann: "__nonnull__".into(),
                        },
                    );
                }
                Token::Ident(s) if s == "satisfies" => {
                    // TS satisfies is type-only; runtime no-op. Parse
                    // and discard the type ann.
                    self.pos += 1;
                    let _ann = self.parse_type_ann()?;
                    continue;
                }
                _ => return Ok(node),
            }
        }
    }

    /// Identifier-or-contextual-keyword name after a `.` / `?.` / for
    /// class member declaration. JS / TS allow reserved words to appear
    /// as property names (`p.catch(...)`, `obj.return`) — routes
    /// keyword tokens through `keyword_property_name` for the full
    /// reserved-word list. Advances `self.pos` on success and returns
    /// None (without consuming) when no name token is present.
    fn member_name_after_dot(&mut self) -> Option<String> {
        // ES §12.7.2 — the name after a dot is an IdentifierName, so an
        // escaped ReservedWord is legal here (`o.break`).
        if let Token::Ident(n) | Token::EscapedIdent(n) = self.peek() {
            let n = n.clone();
            self.pos += 1;
            return Some(n);
        }
        // P8.1 — `#name` PrivateIdentifier after dot. When we are
        // lexically inside a class body (`current_class` is Some) the
        // access is necessarily through `this.#x` or `c.#x` for a `c`
        // typed as the same class — both bind to the SAME class's
        // private slot, so mangling to `__priv_<current_class>__<n>`
        // at parse time is correct and matches the field-decl
        // mangling that A2 wrote into struct_layouts. Cross-class
        // access (e.g. inside D's method, `c: C` and `c.#x`) is
        // rejected by the existing `Visibility::Private` exact-class
        // check in check.rs (M-OO.5 — exact-class match for
        // Private), because the mangled name carries the declaring
        // class in its prefix.
        //
        // Outside any class (`current_class` is None) a `#name`
        // reference is an early SyntaxError per ES §13.1 — leave the
        // token unconsumed so `expect_member_name` refuses it with a
        // targeted message. (S2.37 — the previous "keep the raw
        // `#`-name for the lookup layer" route had rotted into a
        // silent `undefined` through the dynobj member-miss lane.)
        if let Token::PrivateIdent(n) = self.peek() {
            if let Some(cls) = &self.current_class {
                let mangled = format!("__priv_{cls}__{n}");
                self.pos += 1;
                return Some(mangled);
            }
            return None;
        }
        let kw = Self::keyword_property_name(self.peek())?;
        self.pos += 1;
        Some(kw.to_string())
    }

    /// Member name after a just-consumed `.` / `?.`, or the shared
    /// "expected identifier" parse error (`after` names the operator
    /// in the message).
    pub(super) fn expect_member_name(&mut self, after: &str) -> Result<String, String> {
        match self.member_name_after_dot() {
            Some(n) => Ok(n),
            None => {
                // S2.37 — `x.#p` outside any class body: early
                // SyntaxError per ES §13.1 (private references are
                // only legal lexically inside the declaring class).
                if let Token::PrivateIdent(n) = self.peek() {
                    return Err(format!(
                        "private name `#{n}` is only allowed within a class body (at {})",
                        self.at()
                    ));
                }
                let t = self.peek();
                Err(format!(
                    "expected identifier after `{after}`, got {t:?} at {}",
                    self.at()
                ))
            }
        }
    }

    /// Call argument list after a just-consumed `(`: comma-separated
    /// `parse_call_arg`s with the V3-18 trailing-comma wedge (per JS
    /// spec §13.3.6 / ES2017: `f(a, b,)`), through the closing `)`,
    /// then the P5.5 static-spread fold.
    fn parse_call_args(&mut self) -> Result<Vec<ExprId>, String> {
        let mut args = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            args.push(self.parse_call_arg()?);
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                if matches!(self.peek(), Token::RParen) {
                    break;
                }
                args.push(self.parse_call_arg()?);
            }
        }
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
        }
        Ok(self.fold_static_spread(args))
    }

    /// P5.5 — static `f(...[a, b, c])` literal-array spread desugars
    /// to `f(a, b, c)` here so the fixed-arity arity check downstream
    /// doesn't see the spread as a single arg. Dynamic spread
    /// (`f(...someVar)`) only works with rest-param sigs — fixed-arity
    /// dynamic spread requires runtime length checking out of scope
    /// for the typed subset.
    pub(super) fn fold_static_spread(&mut self, args: Vec<ExprId>) -> Vec<ExprId> {
        let needs_spread_fold = args.iter().any(|a| {
            matches!(self.ast.get_expr(*a), Expr::Spread { expr }
                if matches!(self.ast.get_expr(*expr), Expr::Array(_)))
        });
        if !needs_spread_fold {
            return args;
        }
        let mut folded: Vec<ExprId> = Vec::with_capacity(args.len());
        for a in &args {
            if let Expr::Spread { expr } = self.ast.get_expr(*a)
                && let Expr::Array(els) = self.ast.get_expr(*expr)
            {
                for e in els.clone() {
                    folded.push(e);
                }
            } else {
                folded.push(*a);
            }
        }
        folded
    }

    /// Index access `obj[index]` including the V3-18 wedge:
    /// `obj["x"]` ≡ `obj.x` per JS spec §13.3.2 when "x" parses as a
    /// valid identifier. Folding here (vs at typecheck) keeps the
    /// entire downstream pipeline (typecheck / lower / drop /
    /// write-side assign) unchanged: the synthetic Member routes
    /// through every existing field-resolve path, including struct
    /// layouts, refcount on owned fields, and Member-call dispatch.
    /// Only fires for compile-time string literals whose content is a
    /// syntactic IdentifierName; dynamic / numeric / non-identifier
    /// indices stay as Index and hit the existing Array / String paths.
    pub(super) fn parse_postfix_index(
        &mut self,
        node: ExprId,
        start_pos: usize,
    ) -> Result<ExprId, String> {
        self.pos += 1;
        let index = self.parse_expr()?;
        match self.peek() {
            Token::RBracket => self.pos += 1,
            t => return Err(format!("expected `]`, got {t:?} at {}", self.at())),
        }
        let folded = if let Expr::String(name) = self.ast.get_expr(index) {
            if is_identifier_name(name) {
                Some(name.clone())
            } else {
                None
            }
        } else {
            None
        };
        Ok(if let Some(name) = folded {
            self.add_expr_at(start_pos, Expr::Member { obj: node, name })
        } else {
            self.add_expr_at(start_pos, Expr::Index { obj: node, index })
        })
    }

    /// `obj?.[index]` — ES2020 optional element access (chunk 703).
    /// Mirrors [`Self::parse_postfix_index`], including the
    /// string-literal identifier fold: `a?.["k"]` is member access
    /// spelled as element access, so it folds to the existing
    /// OptChain shape and rides every field-resolve path.
    fn parse_optchain_index(&mut self, node: ExprId, start_pos: usize) -> Result<ExprId, String> {
        self.pos += 1;
        let index = self.parse_expr()?;
        match self.peek() {
            Token::RBracket => self.pos += 1,
            t => return Err(format!("expected `]`, got {t:?} at {}", self.at())),
        }
        let folded = if let Expr::String(name) = self.ast.get_expr(index) {
            if is_identifier_name(name) {
                Some(name.clone())
            } else {
                None
            }
        } else {
            None
        };
        Ok(if let Some(name) = folded {
            self.add_expr_at(start_pos, Expr::OptChain { obj: node, name })
        } else {
            self.add_expr_at(start_pos, Expr::OptIndex { obj: node, index })
        })
    }

    /// Is the current `!` a TS non-null assertion (vs the start of a
    /// prefix `!expr`)? Conservative test: peek for tokens that can
    /// NOT start an expression — terminators, operators, statement
    /// boundaries.
    fn bang_is_postfix(&self) -> bool {
        let next = self.tokens.get(self.pos + 1).map(|s| &s.token);
        matches!(
            next,
            Some(Token::Semi)
                | Some(Token::Comma)
                | Some(Token::RParen)
                | Some(Token::RBracket)
                | Some(Token::RBrace)
                | Some(Token::Dot)
                | Some(Token::Eq)
                | Some(Token::Colon)
                | Some(Token::QuestionDot)
                | Some(Token::EqEq)
                | Some(Token::EqEqEq)
                | Some(Token::BangEq)
                | Some(Token::BangEqEq)
                | Some(Token::Plus)
                | Some(Token::Minus)
                | Some(Token::Star)
                | Some(Token::Slash)
                | Some(Token::Percent)
                | Some(Token::Amp)
                | Some(Token::Pipe)
                | Some(Token::Lt)
                | Some(Token::Gt)
                | Some(Token::AmpAmp)
                | Some(Token::PipePipe)
                | Some(Token::Question)
                | Some(Token::FatArrow)
                | Some(Token::LParen)
                | Some(Token::LBracket)
                | Some(Token::Eof)
                | None
        )
    }
}
