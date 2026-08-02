//! Function-expression cluster (chunk 414).
//!
//! Extracted verbatim from parser.rs — the three methods behind
//! function values in expression position:
//! - is_arrow_fn_at_lparen — lookahead from `(` for `=>` (incl.
//!   `(): T => ...` return-ann forms) to disambiguate arrow-fn vs
//!   parenthesized expression
//! - parse_fn_expr — `function (params): R { body }` /
//!   `function NAME(...)` / `function*() {}` stub-drop, all emitted
//!   as Expr::ArrowFn
//! - parse_arrow_fn — `(params) => body` with destr-param /
//!   optional-param / trailing-comma wedges and expression-body →
//!   single-Return desugar
//!
//! All three are called from parse_primary (parser.rs); promoted
//! `pub(super)` per the sibling-impl pack pattern. Body unchanged.

use super::*;

impl<'a> Parser<'a> {
    /// Lookahead: from a `(` at `self.pos`, find the matching `)` and peek
    /// for `=>` (or `: T => ...`) to decide arrow-fn vs parenthesized expression.
    /// Handles nested parens correctly.
    pub(super) fn is_arrow_fn_at_lparen(&self) -> bool {
        debug_assert!(matches!(self.peek(), Token::LParen));
        let mut depth: i32 = 1;
        let mut i = self.pos + 1;
        while i < self.tokens.len() {
            match &self.tokens[i].token {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        // Direct arrow: `() => ...`
                        if matches!(
                            self.tokens.get(i + 1).map(|s| &s.token),
                            Some(Token::FatArrow)
                        ) {
                            return true;
                        }
                        // Arrow with explicit return type: `() : T => ...`
                        // — skip past the `: TypeAnnotation` and look for
                        // `=>`. Type annotation is IDENT followed by
                        // optional `<...>` generics + `[]` array suffixes.
                        if matches!(self.tokens.get(i + 1).map(|s| &s.token), Some(Token::Colon)) {
                            // Scan past the type ann to look for `=>`.
                            let mut j = i + 2;
                            // An inline function type as the return
                            // annotation: `(): (b: number) => void =>
                            // …`. Requiring an identifier here read
                            // its leading `(` as "not an arrow", so
                            // the whole declaration fell into the
                            // parenthesized-expression path and died
                            // on the empty `()` with "expected
                            // expression, got RParen".
                            //
                            // Only the unparenthesized spelling. A
                            // parameter list holds named parameters,
                            // so a group opening with another `(` is
                            // a parenthesized type instead — and that
                            // spelling needs the annotation parser to
                            // stop at the arrow's own `=>`, which is
                            // `parse_fn_type_ann`'s pinning question,
                            // not this lookahead's. Declining keeps
                            // it exactly as it was.
                            if matches!(self.tokens.get(j).map(|s| &s.token), Some(Token::LParen)) {
                                if matches!(
                                    self.tokens.get(j + 1).map(|s| &s.token),
                                    Some(Token::LParen)
                                ) {
                                    return false;
                                }
                                let mut d = 1;
                                j += 1;
                                while j < self.tokens.len() && d > 0 {
                                    match self.tokens[j].token {
                                        Token::LParen => d += 1,
                                        Token::RParen => d -= 1,
                                        Token::Eof => return false,
                                        _ => {}
                                    }
                                    j += 1;
                                }
                                // The `=>` of the function type just
                                // skipped, then its return type, then
                                // the arrow's own `=>`.
                                if !matches!(
                                    self.tokens.get(j).map(|s| &s.token),
                                    Some(Token::FatArrow)
                                ) {
                                    return false;
                                }
                                j += 1;
                            }
                            // Type starts with an identifier — or
                            // the `void` keyword (m1.h.30 promoted
                            // it from contextual ident to keyword;
                            // arrow lookahead must accept it here
                            // for `(): void => ...` to parse).
                            if !matches!(
                                self.tokens.get(j).map(|s| &s.token),
                                Some(Token::Ident(_)) | Some(Token::Void)
                            ) {
                                return false;
                            }
                            j += 1;
                            // Optional generic args `<T1, T2, ...>` —
                            // very rough scan; if we hit a stray `>`
                            // before the next plausible `=>`, fall back
                            // to "not arrow".
                            if matches!(self.tokens.get(j).map(|s| &s.token), Some(Token::Lt)) {
                                let mut g = 1;
                                j += 1;
                                while j < self.tokens.len() && g > 0 {
                                    match self.tokens[j].token {
                                        Token::Lt => g += 1,
                                        Token::Gt => g -= 1,
                                        Token::ShrShr => g -= 2,
                                        Token::Eof => return false,
                                        _ => {}
                                    }
                                    j += 1;
                                }
                            }
                            // Optional `[]` array suffixes.
                            while matches!(
                                self.tokens.get(j).map(|s| &s.token),
                                Some(Token::LBracket)
                            ) && matches!(
                                self.tokens.get(j + 1).map(|s| &s.token),
                                Some(Token::RBracket)
                            ) {
                                j += 2;
                            }
                            // V3-18 wedge — trailing `| null` (the only
                            // union shape this subset's type-ann
                            // parser supports). Allow `() : T | null
                            // => ...` to lookahead-detect as arrow.
                            if matches!(self.tokens.get(j).map(|s| &s.token), Some(Token::Pipe))
                                && matches!(
                                    self.tokens.get(j + 1).map(|s| &s.token),
                                    Some(Token::Null)
                                )
                            {
                                j += 2;
                            }
                            return matches!(
                                self.tokens.get(j).map(|s| &s.token),
                                Some(Token::FatArrow)
                            );
                        }
                        return false;
                    }
                }
                Token::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// `function (params): R { body }` / `function NAME(params): R { body }`
    /// in expression position. Re-uses the FnDecl parser shape but emits
    /// an `Expr::ArrowFn` (the optional self-name is dropped — function
    /// expression names bind only inside the body, a feature out-of-
    /// scope for the subset).
    pub(super) fn parse_fn_expr(&mut self) -> Result<ExprId, String> {
        // current token is `function` — the span anchor (RFC
        // 20260719-fn-tostring-source B1: toString hands back the
        // source slice, so every fn-like node records its byte range).
        let start_pos = self.pos;
        // One-shot: primary_async consumed an `async` prefix for this
        // very call (see the field doc). Taken unconditionally so a
        // stale flag can never leak into an unrelated fn expression.
        let was_async_prefixed = std::mem::take(&mut self.pending_async_fn_expr);
        self.pos += 1;
        // P-PARSE.5 → RFC 20260713-generator-fn-value-substrate blade 2:
        // `function*() {...}` generator function expressions parse for
        // real (params / return ann / body) and the ExprId is marked in
        // `ast.gen_fn_exprs`; the `hoist_gen_fn_exprs` AST pass lifts
        // each marked ArrowFn into a top-level `function* __genexpr_N`
        // decl so the existing generator state-machine desugar handles
        // it. (The pre-blade drop-the-body stub silently ran an empty
        // closure — silent-wrong, now gone.)
        let is_generator = matches!(self.peek(), Token::Star);
        if is_generator {
            self.pos += 1;
        }
        // Optional self-name — chunk 796: recorded by ExprId for the
        // NamedEvaluation registry (`.name` / fn-print; ES §15.5.5 —
        // the self-name wins over a binding name). Body-scope
        // self-binding stays out of scope for the subset.
        let self_name = if let Token::Ident(n) = self.peek() {
            let n = n.clone();
            self.pos += 1;
            Some(n)
        } else {
            None
        };
        let (params, destr_lets) = self.parse_param_list()?;
        // A function *expression* takes plain FormalParameters.
        self.reject_duplicate_params(&params, false)?;
        let mut return_type = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            Some(self.parse_type_ann()?)
        } else {
            None
        };
        // Generator expressions mirror parse_fn's decl-form unwrap:
        // `Generator<T>` / `IterableIterator<T>` / `Iterator<T>` return
        // annotations collapse to the yield type T the state-machine
        // desugar needs (the hoist pass emits the FnDecl with this
        // already-unwrapped ann, so it can't re-run the parser helper).
        if is_generator && let Some(ann) = &return_type {
            let yield_ty = unwrap_generator_return_ann(ann);
            if &yield_ty != ann {
                return_type = Some(yield_ty);
            }
        }
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` after function expression header, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // F2 guard — scope `in_async_gen` to THIS body: true only for
        // an `async function*` expression (the one-shot handshake from
        // primary_async carries the consumed `async`), false for every
        // other function expression, including a sync `function*`
        // nested inside an async-generator body.
        let saved_async_gen =
            std::mem::replace(&mut self.in_async_gen, was_async_prefixed && is_generator);
        let mut stmts = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let s = match self.parse_stmt() {
                Ok(s) => s,
                Err(e) => {
                    self.in_async_gen = saved_async_gen;
                    return Err(e);
                }
            };
            stmts.push(s);
        }
        self.in_async_gen = saved_async_gen;
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` after function expression body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        self.reject_lexical_shadowing_param(&params, &destr_lets, &stmts)?;
        self.reject_use_strict_with_non_simple_params(&params, &stmts)?;
        let destr_prefix = destr_lets.len();
        let stmts = if destr_lets.is_empty() {
            stmts
        } else {
            let mut full = destr_lets;
            full.extend(stmts);
            full
        };
        let eid = self.add_expr_at(
            start_pos,
            Expr::ArrowFn {
                params,
                return_type,
                body: stmts,
            },
        );
        if is_generator {
            self.ast.gen_fn_exprs.insert(
                eid,
                crate::ast::GenFnExprInfo {
                    kind: crate::ast::GenFnExprKind::Generator,
                    destr_prefix,
                },
            );
        } else {
            // RFC 20260717-fnexpr-this-channel knife 1 — the ArrowFn node
            // is a lossy encoding of a function expression: an arrow
            // takes the lexical `this`, a fn-expr binds it at the call
            // site. Record the position so `desugar_fnexpr_this` can give
            // the ones sitting in inline accessor-face positions a
            // `__this` receiver param. (Generator fn-exprs hoist through
            // `hoist_gen_fn_exprs` into decl form instead.)
            self.ast.fn_expr_exprs.insert(eid);
        }
        if let Some(n) = self_name {
            self.ast.fn_expr_self_names.insert(eid, n);
        }
        Ok(eid)
    }

    pub(super) fn parse_arrow_fn(&mut self) -> Result<ExprId, String> {
        // assumes current token is `(` — span anchor (B1, see
        // parse_fn_expr).
        let start_pos = self.pos;
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
                        Some(self.parse_expr()?)
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
                    Some(self.parse_expr()?)
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
        let body = if matches!(self.peek(), Token::LBrace) {
            self.pos += 1;
            let mut stmts = Vec::new();
            while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                stmts.push(self.parse_stmt()?);
            }
            match self.peek() {
                Token::RBrace => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `}}` after arrow fn body, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            stmts
        } else {
            // expression body — desugar to single Return. No stmt
            // boundary of its own exists here, so a hoisted yield
            // would drain OUTSIDE the arrow — and an arrow body is
            // not a yield position anyway (§15.5.5: arrows are not
            // generators): reject via the disallow guard.
            let e = self.with_yield_hoist_disallowed(|p| p.parse_expr())?;
            vec![Stmt::Return(Some(e))]
        };
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

#[cfg(test)]
mod tests {
    use crate::ast::Expr;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    /// Slice `src` by the span of the first `Expr::ArrowFn` the parse
    /// produced (RFC 20260719-fn-tostring-source B1 — toString hands
    /// back this exact byte range, so the span must cover the full
    /// fn-like form).
    fn first_arrow_span_text(src: &str) -> String {
        let tokens = tokenize(src).expect("tokenize");
        let ast = parse(src, &tokens).expect("parse");
        let (eid, _) = ast
            .exprs
            .iter()
            .enumerate()
            .find(|(_, e)| matches!(e, Expr::ArrowFn { .. }))
            .expect("an ArrowFn node");
        let span = &ast.expr_spans[eid];
        assert!(
            span.start != 0 || span.end != 0,
            "ArrowFn span left at the (0,0) sentinel"
        );
        src[span.start as usize..span.end as usize].to_string()
    }

    #[test]
    fn fn_expr_span_covers_keyword_to_body_brace() {
        let text = first_arrow_span_text("const f = function (a: number) { return a; };");
        assert_eq!(text, "function (a: number) { return a; }");
    }

    #[test]
    fn named_fn_expr_span_includes_self_name() {
        let text = first_arrow_span_text("const f = function me(a: number) { return a; };");
        assert_eq!(text, "function me(a: number) { return a; }");
    }

    #[test]
    fn paren_arrow_span_covers_params_to_body() {
        let text = first_arrow_span_text("const g = (x: number) => x * 2;");
        assert_eq!(text, "(x: number) => x * 2");
    }

    #[test]
    fn bare_arrow_span_starts_at_param_ident() {
        let text = first_arrow_span_text("const h = x => x + 1;");
        assert_eq!(text, "x => x + 1");
    }

    #[test]
    fn async_paren_arrow_span_includes_async_prefix() {
        let text = first_arrow_span_text("const i = async (x: number) => x;");
        assert_eq!(text, "async (x: number) => x");
    }

    #[test]
    fn async_bare_arrow_span_includes_async_prefix() {
        let text = first_arrow_span_text("const j = async x => x;");
        assert_eq!(text, "async x => x");
    }

    #[test]
    fn objlit_method_shorthand_span_starts_at_name() {
        let text = first_arrow_span_text("const o = { m(a: number): number { return a; } };");
        assert_eq!(text, "m(a: number): number { return a; }");
    }

    #[test]
    fn objlit_getter_span_starts_at_get_keyword() {
        let text = first_arrow_span_text("const p = { get v(): number { return 1; } };");
        assert_eq!(text, "get v(): number { return 1; }");
    }

    #[test]
    fn multiline_fn_expr_span_preserves_inner_whitespace() {
        let src = "const f = function (a: number, b: number): number {\n  return a + b;\n};\n";
        let text = first_arrow_span_text(src);
        assert_eq!(
            text,
            "function (a: number, b: number): number {\n  return a + b;\n}"
        );
    }
}
