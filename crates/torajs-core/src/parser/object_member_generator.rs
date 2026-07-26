//! Object-literal generator method shorthand — `{ *g() { yield 1 } }`.
//!
//! P-SURF S2.1, one of the grammar point's three positions. The
//! generator substrate itself is already whole: `function* g() {}`
//! runs, and so does the `function*() {}` expression form (parsed as
//! an `Expr::ArrowFn` marked in `ast.gen_fn_exprs`, then lifted to a
//! top-level decl by `hoist_gen_fn_exprs`). Only the parser refused
//! this position, with `expected field name in object literal, got
//! Star`.
//!
//! So this mints nothing new. It mirrors the async shorthand next
//! door (`object_member.rs`) exactly: parse params + body, push a
//! synthetic top-level `Stmt::FnDecl` — here with `is_generator:
//! true`, the field that already exists on the node — and hand the
//! property back an `Expr::Ident` naming it. The generator
//! state-machine desugar then picks it up as if the user had written
//! `function* __obj_gen_method_0() {}` at top level, which is what
//! ES §13.2.5 says the shorthand means.
//!
//! The return annotation goes through `unwrap_generator_return_ann`
//! for the same reason the decl and expression forms do:
//! `Generator<T>` / `IterableIterator<T>` / `Iterator<T>` collapse to
//! the yield type `T` that the desugar consumes.
//!
//! `*[computed]() {}` is deliberately not matched — computed keys ride
//! their own path (`object_literal_computed.rs`), and pairing the two
//! is a separate item. Falling through with `Ok(None)` leaves that
//! shape's existing diagnostic intact rather than half-claiming it.

use super::Parser;
use super::type_ann_helpers::unwrap_generator_return_ann;
use crate::ast::{Expr, ExprId, Stmt};
use crate::lexer::Token;

impl<'a> Parser<'a> {
    /// Try to parse a `*<Ident>(...) {...}` generator method shorthand.
    /// `Ok(Some((field_name, value)))` when the lookahead matched and
    /// the method was consumed whole; `Ok(None)` when the leading token
    /// is not this shape, leaving the caller's other paths untouched.
    pub(super) fn try_parse_generator_object_method(
        &mut self,
    ) -> Result<Option<(String, ExprId)>, String> {
        if !matches!(self.peek(), Token::Star) {
            return Ok(None);
        }
        // Span anchor is the `*` — where the method's source text
        // starts (RFC 20260719-fn-tostring-source B1). Nothing is
        // consumed before the lookahead guards below return None, so
        // capturing it here is safe.
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
        // Consume `*` and the method name.
        self.pos += 2;

        let (mut params, destr_lets) = self.parse_param_list()?;
        self.infer_default_param_anns(&mut params);

        let return_type = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            let ann = self.parse_type_ann()?;
            Some(unwrap_generator_return_ann(&ann))
        } else {
            None
        };

        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` after generator object-method `{method_name}` header, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            body.push(self.parse_stmt()?);
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end generator object-method `{method_name}` body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let body = if destr_lets.is_empty() {
            body
        } else {
            let mut full = destr_lets;
            full.extend(body);
            full
        };

        // Same synth-decl channel the async shorthand uses: the
        // `synth_classes` drain in `parse_program` prepends accumulated
        // synth stmts to the next top-level stmt, which gives the
        // declared-before-use ordering this needs.
        let synth_id = self.mint_desugar_id();
        let synth_name = format!("__obj_gen_method_{synth_id}");
        self.synth_classes.push(Stmt::FnDecl {
            name: synth_name.clone(),
            type_params: Vec::new(),
            params,
            return_type,
            body,
            is_generator: true,
            span: self.span_from(start_pos),
        });

        let value = self.ast.add_expr(Expr::Ident(synth_name));
        Ok(Some((method_name, value)))
    }
}
