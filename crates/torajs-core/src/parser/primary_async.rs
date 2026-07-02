//! Async expression-position forms (chunk 420).
//!
//! Extracted verbatim from parse_primary (parser.rs) — the two async
//! arms, fused into one fall-through probe:
//! - async arrow — `async x => ...` / `async (a, b) => ...` (P1
//!   opaque stub: params + body dropped, empty Expr::ArrowFn)
//! - async function expression — `async function [NAME](...) {...}` /
//!   `async function*` (P0.10 opaque stub, same strategy)
//!
//! Returns Ok(None) when the cursor is not at either form (including
//! `async` used as a plain identifier), so parse_primary falls through
//! to its remaining arms. The only non-verbatim edits are the two
//! `Ok(expr)` -> `Ok(Some(expr))` wraps at the arm tails.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn try_parse_async_expr(&mut self) -> Result<Option<ExprId>, String> {
        // P0.10 — async function expression `async function() {...}` /
        // `async function NAME() {...}` per ES spec §15.8.5
        // AsyncFunctionExpression. Used in test262 for `async function()
        // {}.constructor` (~15+ cases under built-ins/AsyncFunction/* and
        // built-ins/AsyncDisposableStack/*). Real async-fn-expression
        // substrate (state-machine generation, await binding) is a
        // P-LATER item; for the parser milestone we accept the syntax,
        // brace-balance the body, and emit an empty placeholder
        // Expr::ArrowFn — same strategy as generator-expression
        // (function*) and getter/setter/computed-method bodies.
        // P1 — async arrow functions `async x => ...` / `async (a, b)
        // => ...`. tora's regular arrow parser doesn't recognize the
        // `async` prefix in expression position (the keyword is
        // distinct from Ident "async"). Stub the syntax by dropping
        // the body — same opaque-stub strategy as async function
        // expression (real await binding / state-machine substrate
        // is P-LATER). Detected forms:
        //   async Ident => <expr | { body }>
        //   async (Params) => <expr | { body }>
        if matches!(self.peek(), Token::Async)
            && let Some(t1) = self.tokens.get(self.pos + 1)
            && (matches!(t1.token, Token::Ident(_)) || matches!(t1.token, Token::LParen))
        {
            // Distinguish `async function ...` (handled below) from
            // an async arrow. The Function check is later in the if
            // chain — here both Ident and LParen lead to arrow form.
            // For Ident case, peek+2 must be FatArrow.
            let is_arrow = if matches!(t1.token, Token::Ident(_)) {
                self.tokens
                    .get(self.pos + 2)
                    .is_some_and(|t| matches!(t.token, Token::FatArrow))
            } else {
                // LParen — scan to matching RParen, then check for FatArrow
                let mut j = self.pos + 2;
                let mut depth = 1i32;
                while depth > 0 && j < self.tokens.len() {
                    match self.tokens[j].token {
                        Token::LParen => depth += 1,
                        Token::RParen => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                self.tokens
                    .get(j)
                    .is_some_and(|t| matches!(t.token, Token::FatArrow))
            };
            if is_arrow {
                self.pos += 1; // consume `async`
                // Drop the param list / single Ident.
                if matches!(self.peek(), Token::Ident(_)) {
                    self.pos += 1; // single-param shorthand
                } else {
                    // (...)
                    self.pos += 1; // consume LParen
                    let mut depth = 1i32;
                    while depth > 0 {
                        match self.peek() {
                            Token::LParen => depth += 1,
                            Token::RParen => depth -= 1,
                            Token::Eof => {
                                return Err(format!(
                                    "unexpected eof in async arrow params at {}",
                                    self.at()
                                ));
                            }
                            _ => {}
                        }
                        self.pos += 1;
                    }
                }
                // Consume FatArrow.
                self.pos += 1;
                // Body — either expression or block.
                if matches!(self.peek(), Token::LBrace) {
                    self.pos += 1;
                    let mut depth = 1i32;
                    while depth > 0 {
                        match self.peek() {
                            Token::LBrace => depth += 1,
                            Token::RBrace => depth -= 1,
                            Token::Eof => {
                                return Err(format!(
                                    "unexpected eof in async arrow body at {}",
                                    self.at()
                                ));
                            }
                            _ => {}
                        }
                        self.pos += 1;
                    }
                } else {
                    // Expression body — parse and discard.
                    let _ = self.parse_assign()?;
                }
                return Ok(Some(self.ast.add_expr(Expr::ArrowFn {
                    params: Vec::new(),
                    return_type: None,
                    body: Vec::new(),
                })));
            }
        }
        if matches!(self.peek(), Token::Async)
            && let Some(next) = self.tokens.get(self.pos + 1)
            && matches!(next.token, Token::Function)
        {
            self.pos += 2; // consume `async function`
            // P1 — `async function*` (async generator) is also accepted
            // and stubbed via the same drop-the-body strategy. Consume
            // the optional `*` token.
            if matches!(self.peek(), Token::Star) {
                self.pos += 1;
            }
            // Optional name — accept and discard.
            if let Token::Ident(_) = self.peek() {
                self.pos += 1;
            }
            // Drop the param list paren-balanced.
            match self.peek() {
                Token::LParen => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `(` after async function expression, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            let mut depth: i32 = 1;
            while depth > 0 {
                match self.peek() {
                    Token::LParen => depth += 1,
                    Token::RParen => depth -= 1,
                    Token::Eof => {
                        return Err(format!(
                            "unexpected EOF inside async function expression param list at {}",
                            self.at()
                        ));
                    }
                    _ => {}
                }
                self.pos += 1;
            }
            // Optional return-type annotation.
            if matches!(self.peek(), Token::Colon) {
                self.pos += 1;
                let _ = self.parse_type_ann()?;
            }
            // Drop the body brace-balanced.
            match self.peek() {
                Token::LBrace => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `{{` after async function expression header, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            let mut depth: i32 = 1;
            while depth > 0 {
                match self.peek() {
                    Token::LBrace => depth += 1,
                    Token::RBrace => depth -= 1,
                    Token::Eof => {
                        return Err(format!(
                            "unexpected EOF inside async function expression body at {}",
                            self.at()
                        ));
                    }
                    _ => {}
                }
                self.pos += 1;
            }
            return Ok(Some(self.ast.add_expr(Expr::ArrowFn {
                params: Vec::new(),
                return_type: None,
                body: Vec::new(),
            })));
        }
        Ok(None)
    }
}
