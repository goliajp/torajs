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
            return self.parse_array_literal();
        }
        if matches!(self.peek(), Token::LBrace) {
            // `{` in expression position is an object literal. Block
            // statements are caught by `parse_stmt`'s LBrace check before
            // reaching here, so the only path that lands at LBrace in
            // primary is an expression context (let-init, fn arg, return
            // value, etc.).
            return self.parse_object_literal();
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
            Token::Ident(n) => {
                let n = n.clone();
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Ident(n)))
            }
            Token::String(s) => {
                let s = s.clone();
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::String(s)))
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
