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
    /// Optional self-name — chunk 796: recorded by ExprId for the
    /// NamedEvaluation registry (`.name` / fn-print; ES §15.5.5 — the
    /// self-name wins over a binding name). Body-scope self-binding
    /// stays out of scope for the subset.
    fn parse_fn_expr_self_name(&mut self, is_generator: bool) -> Option<String> {
        if let Token::Ident(n) = self.peek() {
            let n = n.clone();
            self.pos += 1;
            return Some(n);
        }
        if matches!(self.peek(), Token::Yield)
            && !is_generator
            && self.class_stack.is_empty()
            && !self.in_strict_fn
        {
            // The one admission site that cannot share
            // `yield_reads_as_ident`: §15.2 names a FunctionExpression's
            // BindingIdentifier [~Yield], so `(function yield() {})` is
            // legal even INSIDE an enclosing generator, and the
            // predicate's clause is the enclosing bit. A generator
            // EXPRESSION's name is [+Yield] (§15.5), which is why this
            // reads its own `is_generator`. The other two clauses are
            // the predicate's, spelled out and kept in step with it.
            let at = self.at();
            self.ast.yield_ident_positions.push(at);
            self.pos += 1;
            return Some("yield".to_string());
        }
        None
    }

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
        let self_name = self.parse_fn_expr_self_name(is_generator);
        // §15.5.1 — FormalParameters ride the fn's OWN [Yield] bit,
        // so the generator swap happens BEFORE the param list (the
        // self-name above stayed on the enclosing scope's bit). Error
        // paths do not restore (failed parse, value moot).
        let saved_gen = std::mem::replace(&mut self.in_generator, is_generator);
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
        let saved_await = std::mem::replace(&mut self.await_allowed, was_async_prefixed);
        // r295 — a generator EXPRESSION body's `this` is the factory
        // call's receiver (§27.5.1.1: OrdinaryCallBindThis on the
        // [[Call]] that mints the generator object). Ride the class
        // generator method's parse-time mint: `this` becomes
        // `Ident(__genrecv)`, and after the body parse a mint that
        // actually fired adds the leading receiver param below. Same
        // known limitation as the class form (parser.rs field doc): a
        // non-arrow fn expression nested in the body inherits the mint.
        let saved_igcm = std::mem::replace(&mut self.in_gen_class_method, is_generator);
        let saved_minted = std::mem::replace(&mut self.gen_recv_minted, false);
        // r334 blade 6 — a function EXPRESSION body is an ordinary
        // function body: own `this`, no [[HomeObject]], SuperProperty
        // inside is an early SyntaxError (§15.4.1) even when the
        // expression sits in a method.
        let saved_super_prop = std::mem::replace(&mut self.super_prop_allowed, false);
        // Same reason, same §10.2.1.2: an ordinary function expression
        // binds its OWN `this`, so a static member body's "this means
        // the class object" recording must not reach inside one. It
        // used to, and `(function () { return this })()` written in a
        // static method died on `closure capture __class_C not in
        // scope` — a name minted for a receiver that expression never
        // had. Arrows keep it (an arrow has no `this` of its own).
        let saved_static_this = self.static_this_class.take();
        let strict_outer = self.in_strict_fn;
        let mut stmts = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let s = match self.parse_stmt() {
                Ok(s) => s,
                Err(e) => {
                    self.await_allowed = saved_await;
                    self.in_generator = saved_gen;
                    self.in_async_gen = saved_async_gen;
                    self.in_gen_class_method = saved_igcm;
                    self.gen_recv_minted = saved_minted;
                    self.super_prop_allowed = saved_super_prop;
                    self.static_this_class = saved_static_this;
                    return Err(e);
                }
            };
            self.arm_strict_directive(&s, &stmts);
            stmts.push(s);
        }
        let recv_minted = self.gen_recv_minted;
        self.await_allowed = saved_await;
        self.in_generator = saved_gen;
        self.in_async_gen = saved_async_gen;
        self.in_gen_class_method = saved_igcm;
        self.gen_recv_minted = saved_minted;
        self.super_prop_allowed = saved_super_prop;
        self.static_this_class = saved_static_this;
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
        // §13.1.1 on the expression's self-name — same deferral as the
        // declaration form: `(function eval() { "use strict"; })` is
        // refused by its own body's directive, which is only known now.
        if let Some(n) = &self_name {
            self.note_strict_binding(n, self.in_strict_fn)?;
        }
        self.finish_fn_body_strict(strict_outer, &params, &mut stmts)?;
        let destr_prefix = destr_lets.len();
        let stmts = if destr_lets.is_empty() {
            stmts
        } else {
            let mut full = destr_lets;
            full.extend(stmts);
            full
        };
        // r295 — the body minted the receiver: prepend the
        // `__genrecv: any = undefined` param. The generator desugar
        // turns it into a `__Gen_*` field like any other param; the
        // wrap forwarder's cell carries FLAG_CLOSURE_RECV_FIRST (the
        // hoist pass registers it) so a method-shaped call seeds the
        // receiver into argv[0], and the `undefined` default covers a
        // detached / direct call (§10.2.1.2, no thisArgument).
        let mut params = params;
        if is_generator && recv_minted {
            let undef = self.ast.add_expr(Expr::Ident("undefined".into()));
            params.insert(
                0,
                crate::ast::Param {
                    name: crate::ast::GEN_RECV_PARAM.into(),
                    type_ann: Some("any".into()),
                    default: Some(undef),
                    is_rest: false,
                },
            );
        }
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
