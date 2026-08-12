//! `Parser::parse_arrow_fn` — paren-form arrow function expressions
//! (`(params) [: R] => body`), moved verbatim from `fn_expr.rs`
//! (rotation 281 file-size split; the await-context knife pushed the
//! parent past the 500-line prod cap). The async paren arrow reaches
//! here through `primary_async`'s one-shot `pending_async_fn_expr`
//! handshake, same shape as `parse_fn_expr`'s.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_arrow_fn(&mut self) -> Result<ExprId, String> {
        // assumes current token is `(` — span anchor (B1, see
        // parse_fn_expr).
        let start_pos = self.pos;
        // One-shot handshake, same shape as parse_fn_expr's: true only
        // when primary_async consumed an `async` prefix for THIS paren
        // arrow. Gates the body's `await` legality below.
        let was_async_prefixed = std::mem::take(&mut self.pending_async_fn_expr);
        self.pos += 1;
        let mut params = Vec::new();
        // V3-18 wedge — destructuring patterns in arrow-fn params,
        // mirror of the parse_fn wedge. `xs.map(([a, b]) => a + b)`
        // is the common shape this unblocks.
        let mut param_destr_lets: Vec<Stmt> = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                if matches!(self.peek(), Token::LBracket | Token::LBrace) {
                    let synth = self.parse_destr_param(&mut param_destr_lets)?;
                    let type_ann = if matches!(self.peek(), Token::Colon) {
                        self.pos += 1;
                        Some(self.parse_type_ann()?)
                    } else {
                        None
                    };
                    // P-PARSE.6 — whole-pattern default on a destr
                    // arrow param: `({a, b} = {a:1, b:2}) => ...`. Per
                    // ES spec §10.2.3 the default fires when the arg
                    // slot is undefined; tora's Param.default plumbs
                    // this through the existing default-arg pipeline,
                    // and the synth binding then carries the
                    // (possibly-defaulted) value into the destr lets.
                    let default = if matches!(self.peek(), Token::Eq) {
                        self.pos += 1;
                        Some(self.with_in_formal_params(|p| p.parse_expr())?)
                    } else {
                        None
                    };
                    // RFC 20260714-dstr-residual — a whole-pattern
                    // default pins the un-annotated synth param to the
                    // default's inferred type (`= {}` → empty Struct),
                    // and the desugared pattern reads then miss at
                    // lower time ("no field" panic). Force `any` — the
                    // catch-destr precedent: reads route the Any tier
                    // and per-field defaults gate correctly.
                    let type_ann = if type_ann.is_none() && default.is_some() {
                        Some("any".to_string())
                    } else {
                        type_ann
                    };
                    params.push(Param {
                        name: synth,
                        type_ann,
                        default,
                        is_rest: false,
                    });
                    match self.peek() {
                        Token::Comma => {
                            self.pos += 1;
                            if matches!(self.peek(), Token::RParen) {
                                break;
                            }
                            continue;
                        }
                        Token::RParen => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `)` after destr param, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
                let pname = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    // §15.3 — ArrowParameters inherit the ENCLOSING
                    // [Yield] bit (arrows never swap it); outside a
                    // generator `yield` is an identifier candidate,
                    // judged by the strict-goal prelude gate.
                    Token::Yield if !self.in_generator && self.class_stack.is_empty() => {
                        let at = self.at();
                        self.ast.yield_ident_positions.push(at);
                        "yield".to_string()
                    }
                    t => {
                        return Err(format!(
                            "expected parameter name, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                // V3-18 wedge — optional parameter in arrow fn:
                // `(x?: T) => ...`. Same modeling as parse_fn.
                let optional = matches!(self.peek(), Token::Question);
                if optional {
                    self.pos += 1;
                }
                let type_ann = if matches!(self.peek(), Token::Colon) {
                    self.pos += 1;
                    let ann = self.parse_type_ann()?;
                    if optional && !ann.starts_with("__nullable(") {
                        Some(format!("__nullable({ann})"))
                    } else {
                        Some(ann)
                    }
                } else {
                    None
                };
                let default = if matches!(self.peek(), Token::Eq) {
                    self.pos += 1;
                    Some(self.with_in_formal_params(|p| p.parse_expr())?)
                } else {
                    // Note: implicit null default for arrow `(x?: T)`
                    // is not synthesized — closure-call lowering of
                    // Nullable<Number> args is currently broken in
                    // ssa_lower (separate pre-existing bug; tracking).
                    // fn-decl + class-method paths are fine and do
                    // synthesize the null default.
                    None
                };
                params.push(Param {
                    name: pname,
                    type_ann,
                    default,
                    is_rest: false,
                });
                match self.peek() {
                    Token::Comma => {
                        self.pos += 1;
                        // V3-18 wedge — trailing comma in arrow-fn params.
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    }
                    Token::RParen => break,
                    t => return Err(format!("expected `,` or `)`, got {t:?} at {}", self.at())),
                }
            }
        }
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
        }
        let return_type = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            Some(self.parse_type_ann()?)
        } else {
            None
        };
        match self.peek() {
            Token::FatArrow => self.pos += 1,
            t => return Err(format!("expected `=>`, got {t:?} at {}", self.at())),
        }
        // ES §15.1.1 duplicate-parameter check, deliberately placed
        // *after* the `=>` rather than after the `)`: until that token
        // is seen the same text may still be a parenthesized sequence
        // expression, and `(x, x)` is perfectly legal as one. Refusing
        // at the `)` would reject the comma operator.
        self.reject_duplicate_params(&params, true)?;
        let saved_await = std::mem::replace(&mut self.await_allowed, was_async_prefixed);
        let body_result = if matches!(self.peek(), Token::LBrace) {
            self.pos += 1;
            let mut stmts = Vec::new();
            let mut err = None;
            while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                match self.parse_stmt() {
                    Ok(s) => stmts.push(s),
                    Err(e) => {
                        err = Some(e);
                        break;
                    }
                }
            }
            match (err, self.peek()) {
                (Some(e), _) => Err(e),
                (None, Token::RBrace) => {
                    self.pos += 1;
                    Ok(stmts)
                }
                (None, t) => Err(format!(
                    "expected `}}` after arrow fn body, got {t:?} at {}",
                    self.at()
                )),
            }
        } else {
            // expression body — desugar to single Return. No stmt
            // boundary of its own exists here, so a hoisted yield
            // would drain OUTSIDE the arrow — and an arrow body is
            // not a yield position anyway (§15.5.5: arrows are not
            // generators): reject via the disallow guard.
            self.with_yield_hoist_disallowed(|p| p.parse_expr())
                .map(|e| vec![Stmt::Return(Some(e))])
        };
        self.await_allowed = saved_await;
        let body = body_result?;
        self.reject_lexical_shadowing_param(&params, &param_destr_lets, &body)?;
        self.reject_use_strict_with_non_simple_params(&params, &body)?;
        // V3-18 wedge — prepend destr-param lets to the body, matching
        // the parse_fn wedge.
        let body = if param_destr_lets.is_empty() {
            body
        } else {
            let mut full = param_destr_lets;
            full.extend(body);
            full
        };
        Ok(self.add_expr_at(
            start_pos,
            Expr::ArrowFn {
                params,
                return_type,
                body,
            },
        ))
    }
}
