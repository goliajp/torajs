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
        // `ArrowFunction : ArrowParameters[?Yield, ?Await] => ConciseBody`
        // — the parameters INHERIT the grammar's `[Await]` parameter,
        // which is what makes `await` reserved inside them wherever it
        // was reserved outside: an async body (§15.8.1) and a class
        // static block (§15.7.1). An `async` prefix forces it on; a
        // plain arrow keeps what the context had, so the two are ORed
        // rather than the context being overwritten.
        //
        // Every other caller of `with_async_params` passes bare
        // asyncness on purpose: a FunctionExpression's are
        // `FormalParameters[~Yield, ~Await]`, unparameterized, so
        // `class C { static { (function (x = await) {}); } }` is legal
        // where the arrow spelling of it is not.
        //
        // `ConciseBody` is NOT parameterized either — the restore
        // below hands the body plain asyncness, which is why
        // `static { () => { var await = 1; } }` stays legal.
        let (params, param_destr_lets, implicit_rest) = self
            .with_async_params(was_async_prefixed || self.await_reserved, |p| {
                p.parse_arrow_param_list()
            })?;
        let return_type = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            // 552-03 — the annotation between here and the body `=>`
            // is the one place a parenthesized composite type must
            // not greedily read as a fn-type parameter list (the
            // arrow's own `=>` sits right behind it).
            let saved = std::mem::replace(&mut self.in_arrow_ret_ann, true);
            let ann = self.parse_type_ann();
            self.in_arrow_ret_ann = saved;
            Some(ann?)
        } else {
            None
        };
        match self.peek() {
            // §15.3 ArrowParameters [no LineTerminator here] `=>`.
            Token::FatArrow if self.has_newline_before(self.pos) => {
                return Err(format!(
                    "a line break is not allowed before `=>` at {} (ES §15.3)",
                    self.at()
                ));
            }
            Token::FatArrow => self.pos += 1,
            t => return Err(format!("expected `=>`, got {t:?} at {}", self.at())),
        }
        // ES §15.1.1 duplicate-parameter check, deliberately placed
        // *after* the `=>` rather than after the `)`: until that token
        // is seen the same text may still be a parenthesized sequence
        // expression, and `(x, x)` is perfectly legal as one. Refusing
        // at the `)` would reject the comma operator.
        self.finish_formal_params(&params, &param_destr_lets, true)?;
        let saved_await = std::mem::replace(&mut self.await_allowed, was_async_prefixed);
        let saved_await_res = std::mem::replace(&mut self.await_reserved, was_async_prefixed);
        let strict_outer = self.in_strict_fn;
        let body_result = if matches!(self.peek(), Token::LBrace) {
            self.parse_arrow_block_body()
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
        self.await_reserved = saved_await_res;
        let mut body = body_result?;
        self.reject_lexical_shadowing_param(&params, &param_destr_lets, &body)?;
        self.reject_use_strict_with_non_simple_params(&params, &body)?;
        // §11.2.2 — an arrow body inside strict code IS strict code,
        // and the directive is how that fact survives the desugar
        // chain (`parser::strict_directive`). This site restored the
        // bit without writing it until rotation 632, on the reasoning
        // that an arrow takes the LEXICAL `this` so nothing downstream
        // would ask this body whether it is strict. Something does:
        // §13.15.2 picks the [[Set]] kernel per FUNCTION
        // (`ssa_lower_set_flavor::cur_fn_is_sloppy`), and it reads the
        // directive off the lifted body — the only carrier left by
        // then. Without one, `function o() { "use strict"; (() => {
        // frozen.a = 1 })() }` took the soft kernel and ran through
        // where node throws.
        //
        // The formatter cost the old note named is paid there instead:
        // an expression-bodied arrow is `[Stmt::Return]`, and
        // `fmt_expr` now skips a synthetic directive before probing
        // for that shape, the same way every other printer site
        // already skips one.
        self.finish_fn_body_strict(strict_outer, &params, &mut body)?;
        // V3-18 wedge — prepend destr-param lets to the body, matching
        // the parse_fn wedge.
        let body = if param_destr_lets.is_empty() {
            body
        } else {
            let mut full = param_destr_lets;
            full.extend(body);
            full
        };
        let eid = self.add_expr_at(
            start_pos,
            Expr::ArrowFn {
                params,
                return_type,
                body,
            },
        );
        if implicit_rest {
            self.ast.implicit_rest_arrows.insert(eid);
        }
        Ok(eid)
    }

    /// The parenthesized parameter list — enters just past the `(`,
    /// leaves past the `)`. Answers the params and any destructuring
    /// prelude lets the body must open with. Extracted from
    /// `parse_arrow_fn` (200-line function limit; the audit's brace
    /// counter had been fooled by a `}}` inside an error string, so
    /// the overage predates 552-03's context flag). The flag answers
    /// whether a rest param took the `any[]` it gets with no
    /// annotation, which a contextual type replaces later.
    fn parse_arrow_param_list(&mut self) -> Result<(Vec<Param>, Vec<Stmt>, bool), String> {
        let mut params = Vec::new();
        let mut implicit_rest = false;
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
                // §15.1 FunctionRestParameter is a BindingRestElement,
                // whose right-hand side is `BindingIdentifier` OR
                // `BindingPattern` — `(...[a, b])` and `(...{length})`
                // are both spelled by the grammar. Reading the rest
                // marker as a reason to stop looking for a pattern
                // rejected them at parse time; the rest slot is an
                // array either way, and the pattern destructures it
                // exactly as it destructures a named one.
                if matches!(self.peek(), Token::LBracket | Token::LBrace) {
                    let synth = self.parse_destr_param(&mut param_destr_lets)?;
                    let type_ann = if matches!(self.peek(), Token::Colon) {
                        self.pos += 1;
                        Some(self.parse_type_ann()?)
                    } else if is_rest {
                        // Untyped rest param is implicitly `any[]`.
                        implicit_rest = true;
                        Some("any[]".into())
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
                        is_rest,
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
                // §15.3 — a RestParameter takes no Initializer; the
                // `!is_rest` gate lets a rest-then-`=` fall through
                // to the loop-tail reject (early error, the
                // dflt-params-rest negative family).
                let default = if !is_rest && matches!(self.peek(), Token::Eq) {
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
                    implicit_rest = true;
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
                        // §15.3 — the rest parameter must be last,
                        // and even a trailing comma after it is an
                        // early error (the rest-params-trailing-comma
                        // negative family; parse_fn's mirror).
                        if is_rest {
                            return Err(format!("rest parameter must be last at {}", self.at()));
                        }
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
        Ok((params, param_destr_lets, implicit_rest))
    }

    /// A `{ … }` arrow body's statement list — enters on the `{`,
    /// leaves past the `}`. Extracted from `parse_arrow_fn` when the
    /// 552-03 return-annotation context flag pushed it over the
    /// 200-line function limit.
    fn parse_arrow_block_body(&mut self) -> Result<Vec<Stmt>, String> {
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
            tk if self.yield_names_here(tk) => {
                let at = self.at();
                self.ast.yield_ident_positions.push(at);
                "yield".to_string()
            }
            // §13.1.1 — `await` is an ordinary BindingIdentifier
            // outside an async body, whatever the strictness; the
            // module-goal half is the recorded site's job.
            tk if self.await_names_here(tk) => self.admit_await_ident(),
            // §12.7.2 — a sloppy arrow parameter may be named `let`.
            Token::Let if self.let_reads_as_ident() => {
                self.record_strict_goal_site("let");
                "let".to_string()
            }
            // `async` and `type` are on nobody's reserved list —
            // `async` is a keyword only where the grammar looks
            // ahead for it and `type` only in TS declaration
            // position, neither of which a binding position is.
            t if Self::contextual_binding_name(t).is_some() => {
                Self::contextual_binding_name(t).unwrap().to_string()
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
