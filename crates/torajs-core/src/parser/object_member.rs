//! Object-literal async method shorthand — real substrate.
//!
//! P10.3-A3b — replaces the pre-existing `parser.rs:4622` stub-drop
//! that dropped the body of `{ async name() { ... } }` and emitted
//! `__async_<n>: null`. Real substrate: parse params + body, mint
//! a unique synthetic `__obj_async_method_<id>` `Stmt::FnDecl`,
//! register it in `ast.async_fns` so `desugar_async` rewrites
//! returns into `Promise.resolve(...)`, and emit the property
//! value as `Expr::Ident(synth_name)` referencing the synth fn.
//!
//! Method shorthand semantics: ES spec §13.2.5 makes `{ f() {} }`
//! sugar for `{ f: function () {} }`. tr's dynobj method-call path
//! (`obj.f(args)`) does not thread `this` into the callee today, so
//! both `function` and `() =>` desugars observe the same
//! `this === undefined` inside the body — picking `Stmt::FnDecl`
//! over `Expr::ArrowFn` here is spec-aligned without altering
//! caller-observable behavior. Distinct from the non-async path at
//! parser.rs:4988 which emits an `Expr::ArrowFn` inline; the async
//! path needs a named `Stmt::FnDecl` because `desugar_async` only
//! reaches stmt-level FnDecls.
//!
//! `async [computedKey]() {}` (computed-property-name method
//! shorthand) stays on the original stub-drop path — Symbol.X
//! dispatch substrate is a P3/P7 follow-up, and the real-rewrite
//! win there is gated on that.

use super::Parser;
use crate::ast::PropKey;
use crate::ast::{Expr, ExprId, Stmt};
use crate::lexer::Token;

impl<'a> Parser<'a> {
    /// Try to parse an `async <Ident>(...) {...}` method shorthand.
    /// Returns `Ok(Some((field_name, value)))` when the lookahead
    /// matched and the method was fully consumed, `Ok(None)` when
    /// the leading token wasn't an async-method shape (caller falls
    /// through to existing paths — incl. computed-key stub-drop and
    /// regular `async` as a field-name ident).
    pub(super) fn try_parse_async_object_method_shorthand(
        &mut self,
    ) -> Result<Option<(PropKey, ExprId)>, String> {
        // Match `async <Ident> (`. Computed-key `async [...]()` stays
        // on the stub-drop path in the caller; explicitly NOT matched
        // here so `Ok(None)` falls through.
        if !matches!(self.peek(), Token::Async) {
            return Ok(None);
        }
        // Span anchor -- the `async` token (B1b); nothing is consumed
        // before the lookahead guards return None, so capturing here
        // is safe.
        let start_pos = self.pos;
        let Some(t1) = self.tokens.get(self.pos + 1) else {
            return Ok(None);
        };
        let Token::Ident(name) = &t1.token else {
            return Ok(None);
        };
        let method_name = name.clone();
        let Some(t2) = self.tokens.get(self.pos + 2) else {
            return Ok(None);
        };
        if !matches!(t2.token, Token::LParen) {
            return Ok(None);
        }
        // Consume `async` and the method name.
        self.pos += 2;

        // Params — reuse the standard param parser (handles
        // destructuring + type anns).
        let (mut params, destr_lets) = self.parse_param_list()?;
        self.reject_duplicate_params(&params, true)?;
        // 刀 1b — method-position default params infer their ann
        // from the default (see param_list.rs).
        self.infer_default_param_anns(&mut params);

        // Optional return-type annotation.
        let return_type = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            Some(self.parse_type_ann()?)
        } else {
            None
        };

        // Body.
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` after async object-method `{method_name}` header, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // §15.8.1 — this IS an async body: await is legal.
        let saved_await = std::mem::replace(&mut self.await_allowed, true);
        let strict_outer = self.in_strict_fn;
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            match self.parse_stmt() {
                Ok(s) => {
                    self.arm_strict_directive(&s, &body);
                    body.push(s);
                }
                Err(e) => {
                    self.await_allowed = saved_await;
                    return Err(e);
                }
            }
        }
        self.await_allowed = saved_await;
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end async object-method `{method_name}` body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        self.reject_use_strict_with_non_simple_params(&params, &body)?;
        self.finish_fn_body_strict(strict_outer, &params, &mut body)?;
        // Prepend destructuring-param helper lets (same shape as
        // non-async method shorthand at parser.rs:4988).
        let body = if destr_lets.is_empty() {
            body
        } else {
            let mut full = destr_lets;
            full.extend(body);
            full
        };

        // Mint synth name + emit synth FnDecl. Reuses `synth_classes`
        // — the field is `Vec<Stmt>`, name's historical (P8.5 class-
        // expression machinery) but the drain in `parse_program` at
        // parser.rs:583 prepends every accumulated synth Stmt to
        // the next emitted top-level stmt, which is exactly the
        // declared-before-use ordering this synth fn needs.
        let synth_id = self.mint_desugar_id();
        let synth_name = format!("__obj_async_method_{synth_id}");
        self.ast.async_fns.insert(synth_name.clone());
        self.synth_classes.push(Stmt::FnDecl {
            name: synth_name.clone(),
            type_params: Vec::new(),
            params,
            return_type,
            body,
            is_generator: false,
            // the synth decl compiles the user's method shorthand --
            // record its source range (B1b)
            span: self.span_from(start_pos),
        });

        // Field value = `Expr::Ident(synth_name)` referencing the
        // synth fn — same shape as `{ greet: greet }` where `greet`
        // is a top-level fn.
        let value = self.ast.add_expr(Expr::Ident(synth_name));
        Ok(Some((PropKey::from(method_name), value)))
    }
}
