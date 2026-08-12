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
//! `*[computed]() {}` is matched here too, which is the one place it
//! can be: the computed-property arm (`object_literal_computed.rs`)
//! dispatches on a leading `[`, and by the time a `[` is current the
//! `*` has already been refused. So neither arm could claim the pair
//! and `{ *[Symbol.iterator]() {} }` — the ordinary way to write an
//! iterable — reached neither. The key is read here and handed the
//! same `__computed_N__` sentinel + `objlit_computed_keys` entry that
//! arm mints, so everything downstream sees one shape, not two.

use super::Parser;
use super::type_ann_helpers::unwrap_generator_return_ann;
use crate::ast::{Expr, ExprId, Param, Stmt};
use crate::lexer::Token;

impl<'a> Parser<'a> {
    /// Try to parse a `*<Ident>(...) {...}` generator method shorthand.
    /// `Ok(Some((field_name, value)))` when the lookahead matched and
    /// the method was consumed whole; `Ok(None)` when the leading token
    /// is not this shape, leaving the caller's other paths untouched.
    pub(super) fn try_parse_generator_object_method(
        &mut self,
    ) -> Result<Option<(String, ExprId)>, String> {
        // P-SURF S2.18 — `{ async *g() {} }` is the same shape with a
        // modifier in front. The async shorthand next door is tried
        // first by the caller and declines it (its name lookahead lands
        // on the `*`), so claiming it here needs no reordering.
        let is_async = matches!(self.peek(), Token::Async)
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.token),
                Some(Token::Star)
            );
        let star_off = if is_async { 1 } else { 0 };
        if !matches!(
            self.tokens.get(self.pos + star_off).map(|t| &t.token),
            Some(Token::Star)
        ) {
            return Ok(None);
        }
        // Span anchor is the `*` — or the `async` before it — where the
        // method's source text starts (RFC 20260719-fn-tostring-source
        // B1). Nothing is consumed before the lookahead guards below
        // return None, so capturing it here is safe.
        let start_pos = self.pos;
        let Some(t1) = self.tokens.get(self.pos + star_off + 1) else {
            return Ok(None);
        };
        // `{ *[Symbol.iterator]() {} }` — a computed key on a generator
        // method. Both key shapes are claimed here rather than next
        // door, because the `*` has to be consumed before the key can
        // be read and the computed-property arm only ever sees a
        // leading `[`. Neither arm could take this alone, which is why
        // it used to reach neither.
        let (method_name, computed_key) = if matches!(t1.token, Token::LBracket) {
            // Consume the optional `async`, the `*`, and the `[`. From
            // here the shape is unambiguous — nothing else in an object
            // literal starts `*[` — so a malformed remainder is an
            // error rather than a decline, which would rewind into a
            // caller that has no path for it either.
            self.pos += star_off + 2;
            let key_expr = self.parse_assign()?;
            match self.peek() {
                Token::RBracket => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `]` after computed generator-method key, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            if !matches!(self.peek(), Token::LParen) {
                return Err(format!(
                    "expected `(` after computed generator-method key, got {:?} at {}",
                    self.peek(),
                    self.at()
                ));
            }
            // The same `__computed_N__` sentinel + `objlit_computed_keys`
            // side table the ordinary computed-property arm mints, at
            // the same point in the parse (after the key, before the
            // body) so the numbering agrees with it.
            (
                format!("__computed_{}__", self.ast.objlit_computed_keys.len()),
                Some(key_expr),
            )
        } else {
            let Token::Ident(name) = &t1.token else {
                return Ok(None);
            };
            let method_name = name.clone();
            let Some(t2) = self.tokens.get(self.pos + star_off + 2) else {
                return Ok(None);
            };
            if !matches!(t2.token, Token::LParen) {
                return Ok(None);
            }
            // Consume the optional `async`, the `*`, and the method name.
            self.pos += star_off + 2;
            (method_name, None)
        };

        // §15.5.1 — the generator bit swaps in BEFORE the param list
        // (FormalParameters[+Yield]); error paths do not restore.
        let saved_gen = std::mem::replace(&mut self.in_generator, true);
        let (mut params, destr_lets) = self.parse_param_list()?;
        self.infer_default_param_anns(&mut params);
        self.reject_duplicate_params(&params, true)?;

        let return_type = self.parse_gen_method_return_ann()?;

        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` after generator object-method `{method_name}` header, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // S2.9 — same as the ordinary object-literal method: `super()`
        // is an early SyntaxError here (ES §15.7.1).
        let saved_super = std::mem::replace(&mut self.super_call_allowed, false);
        // r334 blade 6 — generator object-method: [[HomeObject]] exists,
        // SuperProperty parses (same rationale as the ordinary
        // object-literal method site).
        let saved_super_prop = std::mem::replace(&mut self.super_prop_allowed, true);
        let saved_async_gen = std::mem::replace(&mut self.in_async_gen, is_async);
        let saved_await = std::mem::replace(&mut self.await_allowed, is_async);
        // Knife 4d — arena range for the `arguments` rename sweep
        // below (the class half does the same, see
        // parse_class_decl_generator.rs knife 2b).
        let body_expr_start = self.ast.exprs.len();
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
                    self.in_generator = saved_gen;
                    self.in_async_gen = saved_async_gen;
                    self.super_call_allowed = saved_super;
                    self.super_prop_allowed = saved_super_prop;
                    return Err(e);
                }
            }
        }
        self.await_allowed = saved_await;
        self.in_generator = saved_gen;
        self.in_async_gen = saved_async_gen;
        self.super_call_allowed = saved_super;
        self.super_prop_allowed = saved_super_prop;
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end generator object-method `{method_name}` body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // A destructured parameter (`*g({a, b}) {}`) becomes a synthetic
        // `__param_destr_N` param plus a prefix of `let` statements that
        // unpack it. `desugar_generators` has to peel exactly those into
        // the `__Gen_*` constructor — ES §9.2 binds parameters eagerly, so
        // a throwing destructure must fire at the call, not at the first
        // `next()` — and it finds out how many via this table. Without the
        // entry the lets stay in the body and `__param_destr_N` resolves
        // against a field that was never created.
        self.reject_lexical_shadowing_param(&params, &destr_lets, &body)?;
        self.reject_use_strict_with_non_simple_params(&params, &body)?;
        // Restore only, same reading as the class generator method.
        self.restore_fn_strict(strict_outer, &params)?;
        let destr_prefix = destr_lets.len();
        let body = if destr_lets.is_empty() {
            body
        } else {
            let mut full = destr_lets;
            full.extend(body);
            full
        };

        // Knife 4d (RFC 20260801-arguments-method-face) — shared with
        // the class generator method's knife 2b.
        let body_uses_arguments =
            self.rename_gen_arguments_to_argv(&body, &params, body_expr_start);
        // The argv rides a trailing `any` param — an ordinary
        // generator param, so the __Gen field / ctor / factory
        // plumbing carries it with zero desugar changes. The
        // `__forward_` relay that wraps the hoisted fn's value read
        // fills it with its OWN `[...arguments]` (see the knife-4d
        // arm in ast_closure_param_tag_axes), which the argv/static
        // faces then expand to the true call-site argv.
        if body_uses_arguments {
            params.push(Param {
                name: crate::ast::GEN_ARGV_PARAM.into(),
                type_ann: Some("any".into()),
                default: None,
                is_rest: false,
            });
        }

        // Same synth-decl channel the async shorthand uses: the
        // `synth_classes` drain in `parse_program` prepends accumulated
        // synth stmts to the next top-level stmt, which gives the
        // declared-before-use ordering this needs.
        let synth_id = self.mint_desugar_id();
        let synth_name = format!("__obj_gen_method_{synth_id}");
        if destr_prefix > 0 {
            self.ast
                .gen_param_destr_prefix
                .insert(synth_name.clone(), destr_prefix);
        }
        // Async-ness rides a side table keyed by the declared name, so
        // the synthetic decl is registered exactly as a top-level
        // `async function*` would be — see the class half in
        // `parse_class_decl_generator.rs` for why the set is
        // `async_generator_fns` and not `async_fns`.
        if is_async {
            self.ast.async_generator_fns.insert(synth_name.clone());
        }
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
        if let Some(key_expr) = computed_key {
            self.ast.objlit_computed_keys.insert(value, key_expr);
        }
        Ok(Some((method_name, value)))
    }

    /// The optional `: Generator<T>`-style return annotation of a
    /// generator method, collapsed to the yield type the state-machine
    /// desugar consumes (split out of the method parser above to keep
    /// it under the 200-line fn limit).
    fn parse_gen_method_return_ann(&mut self) -> Result<Option<String>, String> {
        if !matches!(self.peek(), Token::Colon) {
            return Ok(None);
        }
        self.pos += 1;
        let ann = self.parse_type_ann()?;
        Ok(Some(unwrap_generator_return_ann(&ann)))
    }
}
