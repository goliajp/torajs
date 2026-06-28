//! `Parser::parse_postfix` extracted from `parser.rs` (chunk 164).
//!
//! Pre-extract this method was 235 LOC inside `impl Parser` block.
//! Body verbatim moves here as impl-block sibling (same pattern as
//! chunks 162/163's parse_stmt + try_parse_for_of extractions).
//!
//! `parse_postfix` handles JS postfix operators applied to a
//! primary expression: member access (`.`), index access (`[`),
//! call (`(`), postfix `++` / `--`, optional chaining (`?.`),
//! template tagged calls, and non-null assertion (`!`). Body
//! unchanged.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_postfix(&mut self) -> Result<ExprId, String> {
        let start_pos = self.pos;
        let mut node = self.parse_primary()?;
        loop {
            match self.peek() {
                Token::Dot => {
                    self.pos += 1;
                    let name = match self.member_name_after_dot() {
                        Some(n) => n,
                        None => {
                            let t = self.peek();
                            return Err(format!(
                                "expected identifier after `.`, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    };
                    node = self.add_expr_at(start_pos, Expr::Member { obj: node, name });
                }
                Token::QuestionDot => {
                    self.pos += 1;
                    let name = match self.member_name_after_dot() {
                        Some(n) => n,
                        None => {
                            let t = self.peek();
                            return Err(format!(
                                "expected identifier after `?.`, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    };
                    node = self.add_expr_at(start_pos, Expr::OptChain { obj: node, name });
                }
                Token::LParen => {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        args.push(self.parse_call_arg()?);
                        while matches!(self.peek(), Token::Comma) {
                            self.pos += 1;
                            // V3-18 wedge — trailing comma in call args
                            // (per JS spec §13.3.6 / ES2017): `f(a, b,)`.
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
                    // P5.5 — static `f(...[a, b, c])` literal-array
                    // spread desugars to `f(a, b, c)` here so the
                    // fixed-arity arity check downstream doesn't
                    // see the spread as a single arg. Dynamic spread
                    // (`f(...someVar)`) only works with rest-param
                    // sigs — fixed-arity dynamic spread requires
                    // runtime length checking out of scope for the
                    // typed subset.
                    let needs_spread_fold = args.iter().any(|a| {
                        matches!(self.ast.get_expr(*a), Expr::Spread { expr }
                            if matches!(self.ast.get_expr(*expr), Expr::Array(_)))
                    });
                    if needs_spread_fold {
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
                        args = folded;
                    }
                    node = self.add_expr_at(start_pos, Expr::Call { callee: node, args });
                }
                Token::LBracket => {
                    self.pos += 1;
                    let index = self.parse_expr()?;
                    match self.peek() {
                        Token::RBracket => self.pos += 1,
                        t => return Err(format!("expected `]`, got {t:?} at {}", self.at())),
                    }
                    // V3-18 wedge — `obj["x"]` ≡ `obj.x` per JS
                    // spec §13.3.2 when "x" parses as a valid
                    // identifier. Folding here (vs at typecheck)
                    // keeps the entire downstream pipeline
                    // (typecheck / lower / drop / write-side
                    // assign) unchanged: the synthetic Member
                    // routes through every existing field-resolve
                    // path, including struct layouts, refcount on
                    // owned fields, and Member-call dispatch.
                    // Only fires for compile-time string literals
                    // whose content is a syntactic IdentifierName;
                    // dynamic / numeric / non-identifier indices
                    // stay as Index and hit the existing Array /
                    // String paths.
                    let folded = if let Expr::String(name) = self.ast.get_expr(index) {
                        if is_identifier_name(name) {
                            Some(name.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    node = if let Some(name) = folded {
                        self.add_expr_at(start_pos, Expr::Member { obj: node, name })
                    } else {
                        self.add_expr_at(start_pos, Expr::Index { obj: node, index })
                    };
                }
                Token::PlusPlus | Token::MinusMinus => {
                    // Post-increment / post-decrement: `x++` / `x--`.
                    // JS spec: yields the OLD value, then mutates. ssa_lower
                    // handles the temp-and-store-back machinery directly via
                    // `Expr::PostIncr`. (Pre-increment uses `Expr::Assign`
                    // with a `target = target + 1` shape — that's already
                    // covered by the prefix-side parser.)
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
                // `!x` prefix). Conservative test: peek for tokens
                // that can NOT start an expression — terminators,
                // operators, statement boundaries.
                Token::Bang => {
                    let next = self.tokens.get(self.pos + 1).map(|s| &s.token);
                    let postfix_ok = matches!(
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
                    );
                    if !postfix_ok {
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
}
