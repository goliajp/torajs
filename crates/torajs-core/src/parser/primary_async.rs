//! Async expression-position forms (chunk 420; real substrate since
//! RFC 20260713-generator-fn-value-substrate blade 3).
//!
//! One fall-through probe covering:
//! - async arrow — `async x => ...` / `async (a, b) => ...`: parsed
//!   for real as `Expr::ArrowFn` (the paren form delegates to
//!   `parse_arrow_fn`) and marked in `ast.async_fn_value_exprs` so
//!   `desugar_async` rewrites the body in place before the closure
//!   lift. Captures ride the normal `__env` channel.
//! - async function expression — `async function [NAME](...) {...}`:
//!   delegates to `parse_fn_expr` (same ArrowFn emission as its
//!   non-async form) and marks the ExprId the same way.
//! - `async function*` — `parse_fn_expr` registers the generator in
//!   `ast.gen_fn_exprs`; here the kind is flipped to `AsyncGenerator`
//!   so `hoist_gen_fn_exprs` registers the hoisted name in
//!   `async_generator_fns` (blade 4 decl machinery: factory returns
//!   the generator object, step methods return Promises).
//!
//! Returns Ok(None) when the cursor is not at either form (including
//! `async` used as a plain identifier), so parse_primary falls through
//! to its remaining arms.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn try_parse_async_expr(&mut self) -> Result<Option<ExprId>, String> {
        // Async arrow — `async Ident => ...` / `async (Params) => ...`.
        // The `async` keyword token is distinct from Ident "async", so
        // lookahead must confirm the `=>` before committing (plain
        // `async` as an identifier falls through).
        if matches!(self.peek(), Token::Async)
            && let Some(t1) = self.tokens.get(self.pos + 1)
            && (matches!(t1.token, Token::Ident(_)) || matches!(t1.token, Token::LParen))
        {
            let is_arrow = if matches!(t1.token, Token::Ident(_)) {
                self.tokens
                    .get(self.pos + 2)
                    .is_some_and(|t| matches!(t.token, Token::FatArrow))
            } else {
                // LParen — reuse the arrow lookahead from the `(`
                // position (handles both `(...) =>` and the
                // return-annotated `(...): T =>`; a plain `async(...)`
                // call — e.g. the middle of a ternary — stays false).
                self.pos += 1;
                let r = self.is_arrow_fn_at_lparen();
                self.pos -= 1;
                r
            };
            if is_arrow {
                // Span anchor — the `async` token (B1; the paren form
                // delegates to parse_arrow_fn whose own anchor sits on
                // `(`, so both sub-branches re-anchor below).
                let start_pos = self.pos;
                self.pos += 1; // consume `async`
                let eid = if let Token::Ident(pname) = self.peek() {
                    // Single-param shorthand: `async x => body`.
                    let pname = pname.clone();
                    self.pos += 1; // consume param ident
                    self.pos += 1; // consume `=>` (lookahead-guaranteed)
                    // §15.8.1 — this IS an async body: await is legal.
                    let saved_await = std::mem::replace(&mut self.await_allowed, true);
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
                                "expected `}}` after async arrow body, got {t:?} at {}",
                                self.at()
                            )),
                        }
                    } else {
                        self.parse_expr().map(|e| vec![Stmt::Return(Some(e))])
                    };
                    self.await_allowed = saved_await;
                    let body = body_result?;
                    self.ast.add_expr(Expr::ArrowFn {
                        params: vec![Param {
                            name: pname,
                            type_ann: None,
                            default: None,
                            is_rest: false,
                        }],
                        return_type: None,
                        body,
                    })
                } else {
                    // Paren form — parse_arrow_fn assumes the cursor
                    // sits on `(` and handles params / return ann /
                    // `=>` / body. The one-shot handshake marks its
                    // body async so `await` is legal there (§15.8.1).
                    self.pending_async_fn_expr = true;
                    self.parse_arrow_fn()?
                };
                self.respan_expr(eid, start_pos);
                self.ast.async_fn_value_exprs.insert(eid);
                return Ok(Some(eid));
            }
        }
        // `async function [*] [NAME](...) {...}` — consume `async`,
        // then delegate to parse_fn_expr (cursor on `function`), which
        // handles the optional `*`, self-name, params, return-ann
        // unwrap and body exactly as for the non-async forms.
        if matches!(self.peek(), Token::Async)
            && let Some(next) = self.tokens.get(self.pos + 1)
            && matches!(next.token, Token::Function)
        {
            let start_pos = self.pos;
            self.pos += 1; // consume `async` only
            // F2 guard — an `async function*` EXPRESSION body is an
            // async-generator body: the generic `yield*` desugar must
            // not claim it (rotation 242 sweep audit: the un-flagged
            // expression form rode the sync drive — 65 coincidental
            // passes + 36 mis-drives). One-shot handshake instead of
            // setting `in_async_gen` here: parse_fn_expr scopes the
            // flag to ITS body, so a sync `function*` nested inside
            // does not inherit the refusal.
            self.pending_async_fn_expr = true;
            let eid = self.parse_fn_expr()?;
            // parse_fn_expr anchored at `function`; pull the span back
            // to cover the `async ` prefix (B1).
            self.respan_expr(eid, start_pos);
            if let Some(info) = self.ast.gen_fn_exprs.get_mut(&eid) {
                // `async function*` — flip Generator → AsyncGenerator
                // so the hoist pass wires blade 4's decl machinery.
                info.kind = crate::ast::GenFnExprKind::AsyncGenerator;
            } else {
                self.ast.async_fn_value_exprs.insert(eid);
            }
            return Ok(Some(eid));
        }
        Ok(None)
    }
}
