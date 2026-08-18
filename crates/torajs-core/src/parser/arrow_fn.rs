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
                // §15.3 rest parameter — `(...args) => ...`, the
                // parse_fn wedge's arrow mirror (cluster #13,
                // rotation 442). A rest pattern param stays the
                // named-fn path's reject.
                let is_rest = matches!(self.peek(), Token::DotDotDot);
                if is_rest {
                    self.pos += 1;
                }
                if !is_rest && matches!(self.peek(), Token::LBracket | Token::LBrace) {
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
                let pname = self.expect_arrow_param_name()?;
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
                // An unannotated rest defaults to `any[]` — the
                // collected value IS an array, and a fresh implicit-
                // generic TypeVar here mis-lowers the closure (the
                // destr-default force-`any` precedent above).
                let type_ann = if is_rest && type_ann.is_none() {
                    Some("any[]".to_string())
                } else {
                    type_ann
                };
                params.push(Param {
                    name: pname,
                    type_ann,
                    default,
                    is_rest,
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
        let strict_outer = self.in_strict_fn;
        let body_result = if matches!(self.peek(), Token::LBrace) {
            self.pos += 1;
            let mut stmts = Vec::new();
            let mut err = None;
            while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                match self.parse_stmt() {
                    Ok(s) => {
                        self.arm_strict_directive(&s, &stmts);
                        stmts.push(s);
                    }
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
            //
            // A class expression minted while the body parsed must
            // land INSIDE this body (watermark drain, same protocol
            // as the parse_stmt wrapper) — an expression body has no
            // statement boundary of its own, so without the drain the
            // synth ClassDecl would surface in front of the ENCLOSING
            // statement, where the arrow's parameters are not in
            // scope (406-01). The body then converges on the
            // block-bodied shape `{ class …; return … }`, which the
            // nested-class machinery already handles.
            let synth_mark = self.synth_classes_local.len();
            self.with_yield_hoist_disallowed(|p| p.parse_expr())
                .map(|e| {
                    let mut v = self.synth_classes_local.split_off(synth_mark);
                    v.push(Stmt::Return(Some(e)));
                    v
                })
        };
        self.await_allowed = saved_await;
        let body = body_result?;
        self.reject_lexical_shadowing_param(&params, &param_destr_lets, &body)?;
        self.reject_use_strict_with_non_simple_params(&params, &body)?;
        // Restore only — an arrow takes the LEXICAL `this` (§15.3), so
        // no receiver decision downstream ever asks this body whether
        // it is strict; the enclosing function already answered. It
        // still had to ARM the bit above, because `() => { "use
        // strict"; function f() {} }` makes `f` strict. Writing the
        // directive in here would also cost real ground: an
        // expression-bodied arrow is `[Stmt::Return]` (plus any
        // drained class synths in front — the Return still closes the
        // body), a shape the formatter probes for.
        self.restore_fn_strict(strict_outer, &params)?;
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

    /// Per-param-name admission for the arrow list — Ident, the
    /// sloppy `yield` / `let` spellings, reject otherwise. §15.3 —
    /// ArrowParameters inherit the ENCLOSING [Yield] bit (arrows
    /// never swap it), so this reads whatever bit is in force.
    /// `param_list.rs` carries the same match for the fn-own-bit
    /// callers — a shared extraction is blocked on its file budget
    /// (480/500 at rotation 442).
    fn expect_arrow_param_name(&mut self) -> Result<String, String> {
        let pname = match self.peek() {
            Token::Ident(n) => n.clone(),
            Token::Yield if self.yield_reads_as_ident() => {
                let at = self.at();
                self.ast.yield_ident_positions.push(at);
                "yield".to_string()
            }
            // §12.7.2 — a sloppy arrow parameter may be named `let`.
            Token::Let if self.let_reads_as_ident() => {
                self.record_strict_goal_site("let");
                "let".to_string()
            }
            t => {
                return Err(format!(
                    "expected parameter name, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        Ok(pname)
    }
}
