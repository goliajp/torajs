//! parse_primary dispatcher + array/call-arg cluster (chunk 424).
//!
//! Moved verbatim from parser.rs:
//! - parse_primary — thin dispatcher over the sibling delegates
//!   (primary_atoms / primary_async / primary_new_super / fn_expr /
//!   object_literal) plus the literal-token match
//! - parse_array_literal / parse_array_element — `[1, , 3]` incl.
//!   elision + spread slots
//! - parse_call_arg — call-site slot, same spread shape
//!
//! parse_primary / parse_call_arg are pub(super) for parse_postfix;
//! the array helpers stay module-private. Bodies unchanged.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_primary(&mut self) -> Result<ExprId, String> {
        // Dynamic import / parenthesized-or-arrow — extracted to
        // parser/primary_atoms.rs (chunk 422).
        if matches!(self.peek(), Token::Import) {
            return self.parse_primary_dyn_import();
        }
        if matches!(self.peek(), Token::LParen) {
            return self.parse_primary_paren();
        }
        if matches!(self.peek(), Token::LBracket) {
            // §13.2.4 — every ArrayLiteral element is
            // AssignmentExpression[+In]: the for-head restriction does
            // not reach inside a literal (the paren reset's sibling).
            let saved_in_for_init = std::mem::replace(&mut self.in_for_init, false);
            let r = self.parse_array_literal();
            self.in_for_init = saved_in_for_init;
            return r;
        }
        if matches!(self.peek(), Token::LBrace) {
            // `{` in expression position is an object literal. Block
            // statements are caught by `parse_stmt`'s LBrace check before
            // reaching here, so the only path that lands at LBrace in
            // primary is an expression context (let-init, fn arg, return
            // value, etc.).
            //
            // §13.2.5 — every PropertyDefinition value and every
            // ComputedPropertyName key is [+In] (the
            // accessor-name-computed-in regression: `for (o = { get
            // ['x' in e]() {} };;)` is legal).
            let saved_in_for_init = std::mem::replace(&mut self.in_for_init, false);
            let r = self.parse_object_literal();
            self.in_for_init = saved_in_for_init;
            return r;
        }
        // Function expression — `function (params): R { body }` or
        // `function NAME(params): R { body }` in expression position.
        // IIFE pattern `(function() { ... }())` is the dominant test262
        // shape this unblocks. Treat it as an `Expr::ArrowFn`: lifted by
        // `lift_arrow_fns` to a top-level FnDecl, same downstream
        // pipeline as `() => { ... }`. The optional name is parsed
        // (and ignored — fn-expr names are scoped only to the body, a
        // niche we don't implement).
        if matches!(self.peek(), Token::Function) {
            return self.parse_fn_expr();
        }
        // Class expression (P8.5) — extracted to parser/primary_atoms.rs
        // (chunk 422).
        if matches!(self.peek(), Token::Class) {
            return self.parse_primary_class_expr();
        }
        // Async expression forms (`async ... =>` arrow / `async function`)
        // — extracted to parser/primary_async.rs (chunk 420).
        if let Some(e) = self.try_parse_async_expr()? {
            return Ok(e);
        }
        // Regex literal `/pattern/flags`. The lexer already
        // disambiguated regex vs division by inspecting the previous
        // token; the parser just unwraps the carried pattern + flags
        // into the AST node. check.rs rejects the resulting Expr::Regex
        // with a "regex literals not yet implemented" message — the
        // matching engine is a follow-up phase. Parsing accept here
        // unblocks the lex / parse error buckets ahead of that work.
        if let Token::Regex { pattern, flags } = self.peek().clone() {
            self.pos += 1;
            return Ok(self.ast.add_expr(Expr::Regex { pattern, flags }));
        }
        let pos = self.pos;
        // Single-param arrow `x => body` — extracted to
        // parser/primary_atoms.rs (chunk 422).
        if let Some(e) = self.try_parse_bare_arrow()? {
            return Ok(e);
        }
        match &self.tokens[pos].token {
            // §12.7.2 — the future reserved words reach the parser as
            // plain identifiers, and unlike `eval` / `arguments` they
            // are refused in EVERY occurrence of strict code, this
            // reference position included. Sloppy script code keeps
            // them as ordinary identifiers, so the site is recorded
            // rather than refused and the goal gate answers the module
            // half.
            Token::Ident(n) => {
                let n = n.clone();
                self.note_strict_reference(&n)?;
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Ident(n)))
            }
            // §12.7.2 — where `yield` still reads as a name, it parses
            // plain here and the site is recorded for the strict-GOAL
            // gate (`ast::triage_yield_idents`), the goal bit being
            // stamped after the parse. What reserves it instead is the
            // one predicate every admission site shares.
            tk if self.yield_names_here(tk) => {
                let at = self.at();
                self.ast.yield_ident_positions.push(at);
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Ident("yield".to_string())))
            }
            // §13.1.1 — the same shape for `await`, on the goal axis:
            // the word reads as a name wherever the grammar's `[Await]`
            // parameter is off, and the recorded site is what
            // `ast::triage_await_idents` raises on under the module
            // goal. Reached only from the two callers that already
            // decided no AwaitExpression can stand here — `parse_unary`
            // sends the name-only positions down `parse_postfix`, so
            // this arm never competes with the operator reading.
            tk if self.await_names_here(tk) => {
                let n = self.admit_await_ident();
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Ident(n)))
            }
            // §12.7.2 — the same for `let` in an IdentifierReference.
            // Statement position never arrives here: the statement
            // dispatcher claims `Token::Let` as a declaration before
            // any expression parse begins, and the one shape that
            // falls back out of it carries its own ASI answer
            // (`let_newline_asi_form`).
            Token::Let if self.let_reads_as_ident() => {
                self.record_strict_goal_site("let");
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Ident("let".to_string())))
            }
            Token::String(s) => {
                let s = s.clone();
                let span = self.tokens[self.pos].span;
                self.pos += 1;
                let escaped = s == "use strict"
                    && self.source[span.start as usize..span.end as usize].contains('\\');
                let e = self.ast.add_expr(Expr::String(s));
                if escaped {
                    self.ast.escaped_use_strict.insert(e);
                }
                Ok(e)
            }
            Token::Template { parts } => {
                let parts = parts.clone();
                self.pos += 1;
                self.lower_template_parts(&parts)
            }
            Token::Number(n) => {
                let n = *n;
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Number(n)))
            }
            Token::BigInt { digits, radix } => {
                let digits = digits.clone();
                let radix = *radix;
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::BigInt { digits, radix }))
            }
            Token::True => {
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Bool(true)))
            }
            Token::False => {
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Bool(false)))
            }
            Token::Null => {
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Null))
            }
            Token::This => {
                self.pos += 1;
                // P-SURF S2.1 — inside a hoisted class generator method
                // the receiver arrives as a parameter, and it has to be
                // named here rather than left to
                // `desugar_classes`: that pass runs after
                // `desugar_generators`, which by then has turned `this`
                // into a reference to the `__Gen_*` state-machine
                // instance. See `Parser::in_gen_class_method`.
                if self.in_gen_class_method {
                    let recv = crate::ast::GEN_RECV_PARAM;
                    // r295 — the fn-expression generator form reads
                    // this to know its body actually used `this` (and
                    // so needs the leading `__genrecv` param).
                    self.gen_recv_minted = true;
                    return Ok(self.ast.add_expr(Expr::Ident(recv.into())));
                }
                // P-SURF S2.37, reshaped by RFC 20260804-fn-this-channel
                // knife 2 — inside a static member body `this` is the
                // constructor object (ES §15.7.14). The class-name mint
                // moved down to `desugar_classes` pass 2 (driven by
                // `ast.static_this_sites`) so the AST keeps a real
                // `Expr::This` node a receiver-generic twin can later
                // rebind. Checked after the generator arm: a static
                // generator body's receiver already arrives as
                // GEN_RECV_PARAM.
                if let Some(cls) = self.static_this_class.clone() {
                    let eid = self.ast.add_expr(Expr::This);
                    self.ast.static_this_sites.insert(eid, cls);
                    return Ok(eid);
                }
                Ok(self.ast.add_expr(Expr::This))
            }
            Token::Super => self.parse_primary_super(),
            Token::New => self.parse_primary_new(),
            // A contextual keyword here is an ordinary
            // IdentifierReference. The forms that make `async` a
            // keyword — `async function`, `async x =>`,
            // `async (x) =>` — were tried and declined above
            // (`try_parse_async_expr` returns None for exactly this
            // case, which its doc already calls "`async` used as a
            // plain identifier"), and `type` is TypeScript's only in
            // declaration position, which `parse_stmt` dispatches
            // before any expression is parsed.
            t if Self::contextual_binding_name(t).is_some() => {
                let name = Self::contextual_binding_name(&self.tokens[pos].token).unwrap();
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Ident(name.into())))
            }
            t => Err(format!(
                "expected expression, got {t:?} at {}",
                self.tokens[pos].span.start
            )),
        }
    }

    fn parse_array_literal(&mut self) -> Result<ExprId, String> {
        // assumes current token is `[`
        self.pos += 1;
        let mut elements = Vec::new();
        // P-PARSE.1 — sparse array literal `[1, , 3]`. A comma in the
        // element position is an elision; per ES spec §13.2.4 it
        // contributes one slot whose value is `undefined`. Pre-fix
        // tora's parser bailed at the comma with 'expected expression,
        // got Comma'. The elision synthesized an `Expr::Null`
        // placeholder while Type::Undefined didn't exist yet
        // (pre-P1); RFC 20260714-dstr-residual switched it to real
        // `undefined` — observable through destructuring defaults
        // (`f([,])` must fire `x = 23`; null must not) and hole reads.
        let parse_elem_or_elision = |this: &mut Self| -> Result<ExprId, String> {
            if matches!(this.peek(), Token::Comma | Token::RBracket) {
                // A dedicated Elision node — it reads as undefined
                // but lower_array marks the slot a HOLE (not an own
                // property: `1 in [0,,2]` is false and indexOf skips
                // it per §23.1.3.30 HasProperty gating).
                return Ok(this.ast.add_expr(Expr::Elision));
            }
            this.parse_array_element()
        };
        let mut trailing_after_rest = false;
        if !matches!(self.peek(), Token::RBracket) {
            elements.push(parse_elem_or_elision(self)?);
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                if matches!(self.peek(), Token::RBracket) {
                    // Trailing comma allowed as an expression; when it
                    // follows a rest element, record it — the
                    // assignment-pattern re-read must reject (§13.15.1,
                    // see `arrlit_trailing_comma_after_rest`).
                    trailing_after_rest = elements
                        .last()
                        .is_some_and(|&e| matches!(self.ast.get_expr(e), Expr::Spread { .. }));
                    break;
                }
                elements.push(parse_elem_or_elision(self)?);
            }
        }
        match self.peek() {
            Token::RBracket => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `]` in array literal, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let id = self.ast.add_expr(Expr::Array(elements));
        if trailing_after_rest {
            self.ast.arrlit_trailing_comma_after_rest.insert(id);
        }
        Ok(id)
    }

    /// One slot inside an array literal — either a spread `...src` or a
    /// regular expression. Spread is wrapped in `Expr::Spread { expr }`
    /// so ssa_lower's Array arm can fork into the pre-sized alloc path.
    fn parse_array_element(&mut self) -> Result<ExprId, String> {
        if matches!(self.peek(), Token::DotDotDot) {
            self.pos += 1;
            let inner = self.parse_expr()?;
            return Ok(self.ast.add_expr(Expr::Spread { expr: inner }));
        }
        self.parse_expr()
    }

    /// One arg inside a Call expression — same shape as parse_array_element
    /// so `f(...arr)` parses to `f(Expr::Spread { expr: arr })`. The
    /// `apply_rest_args` AST pass handles the call-site lowering.
    pub(super) fn parse_call_arg(&mut self) -> Result<ExprId, String> {
        if matches!(self.peek(), Token::DotDotDot) {
            self.pos += 1;
            let inner = self.parse_expr()?;
            return Ok(self.ast.add_expr(Expr::Spread { expr: inner }));
        }
        self.parse_expr()
    }
}
