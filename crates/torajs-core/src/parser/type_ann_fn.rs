//! Function-type annotations — `(p: T) => R` and the method-signature
//! spelling `m(p: T): R` that means the same thing.
//!
//! Both produce the `__fn(P|..)->R` encoding every downstream consumer
//! already reads, so a method signature in an inline object type is
//! sugar and nothing below the parser learns a new shape. The two
//! spellings differ only in their tail — `=>` versus `:` — which is why
//! the parameter list is a shared routine and the callers own the rest.
//!
//! Split out of `type_ann.rs` (2026-07-27): that file sat 27 lines under
//! the size limit, so growing it in place was not available.

use super::Parser;
use crate::lexer::Token;

impl Parser<'_> {
    /// `m(p: T): R` in an inline object type — a MethodSignature, which
    /// TS reads as the property `m` holding a `(p: T) => R`. Enters with
    /// the current token on `(`, having already consumed the name.
    ///
    /// An omitted return annotation is `void`, not inferred: there is no
    /// body here to infer from.
    pub(super) fn parse_method_sig_type_ann(&mut self) -> Result<String, String> {
        let (params, _) = self.parse_fn_type_param_list()?;
        let ret = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            self.parse_type_ann()?
        } else {
            "void".to_string()
        };
        Ok(format!("__fn({})->{}", params.join("|"), ret))
    }

    pub(super) fn parse_fn_type_ann(&mut self) -> Result<String, String> {
        let (mut params, fn_shape_pinned) = self.parse_fn_type_param_list()?;
        match self.peek() {
            Token::FatArrow => {
                // 552-03 — in an ARROW FN's return annotation (and
                // only at its top level), `(X) => …` where X cannot
                // be a parameter name (a composite type: `(() =>
                // any)`, `(T[])`) is TS ParenthesizedType even with a
                // `=>` right behind it — that arrow is the enclosing
                // arrow fn's body arrow. Everywhere else the
                // bare-type parameter grammar makes the greedy
                // fn-type read correct (`f: ((n: number) => number)
                // => number` is a fn-type taking a fn), so the
                // re-read is gated on the context flag + depth. A
                // lone bare ident stays a fn-type parameter name
                // (`(x) => void`, matching TS), and a `name:` label
                // or rest param pinned the shape already.
                if self.in_arrow_ret_ann
                    && self.type_ann_depth == 1
                    && params.len() == 1
                    && !fn_shape_pinned
                    && !params[0].chars().all(|c| c.is_alphanumeric() || c == '_')
                {
                    let inner = params.pop().expect("single grouped type");
                    return self.read_type_postfix(inner);
                }
                self.pos += 1;
            }
            t => {
                // Chunk 735 — TS ParenthesizedType: `(T)` with no
                // `=>` after the close paren is a grouped type, most
                // commonly a fn-type array `(() => string)[]`. Only a
                // single bare type re-reads this way — a `name:`
                // label or rest param pinned the fn-type shape and
                // keeps the loud error.
                if params.len() == 1 && !fn_shape_pinned {
                    let inner = params.pop().expect("single grouped type");
                    return self.read_type_postfix(inner);
                }
                return Err(format!(
                    "expected `=>` in fn-type, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let ret = self.parse_type_ann()?;
        Ok(format!("__fn({})->{}", params.join("|"), ret))
    }

    /// The parenthesised parameter list both spellings share. Enters on
    /// `(`, leaves just past `)`. The bool answers whether anything
    /// pinned this as a fn-type rather than a parenthesised type — only
    /// the `=>` caller can act on it, but it is computed here.
    fn parse_fn_type_param_list(&mut self) -> Result<(Vec<String>, bool), String> {
        // current token = `(`
        self.pos += 1;
        let mut params: Vec<String> = Vec::new();
        // Chunk 735 — a `name:` label or rest param pins the shape as
        // a fn-type; without either, a single parenthesized type can
        // re-read as TS ParenthesizedType when no `=>` follows the
        // close paren (`(() => string)[]`).
        let mut fn_shape_pinned = false;
        if !matches!(self.peek(), Token::RParen) {
            loop {
                // `...name: E[]` rest param (RFC 20260708-variadic) —
                // TS grammar: must be last, must be an array type.
                // Encodes as `__rest(E[])` in the param slot.
                if matches!(self.peek(), Token::DotDotDot) {
                    self.pos += 1;
                    fn_shape_pinned = true;
                    let name_then_colon = matches!(self.peek(), Token::Ident(_))
                        && matches!(
                            self.tokens.get(self.pos + 1).map(|s| &s.token),
                            Some(Token::Colon)
                        );
                    if name_then_colon {
                        self.pos += 2;
                    }
                    let pty = self.parse_type_ann()?;
                    if !(pty.ends_with("[]") || (pty.starts_with("Array<") && pty.ends_with('>'))) {
                        return Err(format!(
                            "rest param type must be an array type, got `{pty}` at {}",
                            self.at()
                        ));
                    }
                    params.push(format!("__rest({pty})"));
                    match self.peek() {
                        Token::RParen => break,
                        t => {
                            return Err(format!(
                                "rest param must be last in fn-type params, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
                // Optional `name:` prefix on each param. Name is discarded;
                // we keep only the type. Three shapes accepted:
                //   `name: T`  — TS standard fn-type form.
                //   `name?: T` — optional param (424-01). Encodes as
                //     `__nullable(T)`, the SAME spelling a value-side
                //     `(b?: string) =>` parameter carries (see
                //     `param_optional_default.rs`) — the two faces of
                //     one annotation must not disagree, and refusing
                //     the SPELLING made every program carrying a
                //     TS-idiomatic callback annotation a parse error.
                //     The arity face (a shorter call through the
                //     slot) is checker territory — 424-01 residual.
                //   `T`        — bare type, no name (fallback).
                let name_then_colon = matches!(self.peek(), Token::Ident(_))
                    && matches!(
                        self.tokens.get(self.pos + 1).map(|s| &s.token),
                        Some(Token::Colon)
                    );
                let name_question_colon = matches!(self.peek(), Token::Ident(_))
                    && matches!(
                        self.tokens.get(self.pos + 1).map(|s| &s.token),
                        Some(Token::Question)
                    )
                    && matches!(
                        self.tokens.get(self.pos + 2).map(|s| &s.token),
                        Some(Token::Colon)
                    );
                if name_question_colon {
                    self.pos += 3;
                    fn_shape_pinned = true;
                } else if name_then_colon {
                    self.pos += 2;
                    fn_shape_pinned = true;
                }
                let pty = self.parse_type_ann()?;
                let pty = if name_question_colon && !pty.starts_with("__nullable(") {
                    format!("__nullable({pty})")
                } else {
                    pty
                };
                params.push(pty);
                match self.peek() {
                    Token::Comma => self.pos += 1,
                    Token::RParen => break,
                    t => {
                        return Err(format!(
                            "expected `,` or `)` in fn-type params, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
            }
        }
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
        }
        Ok((params, fn_shape_pinned))
    }
}
