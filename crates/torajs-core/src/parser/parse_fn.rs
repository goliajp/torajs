//! `function` declaration parser (chunk 423).
//!
//! parse_fn moved verbatim from parser.rs and decomposed into three
//! functions so each stays under the 200-line hard limit:
//! - parse_fn — header (generator `*`, name), then delegates, then
//!   return type / generator yield-type recording / overload
//!   signature discard / body / FnDecl assembly
//! - parse_fn_type_params — `<T, U>` list (M3)
//! - parse_fn_params — `(a: T, [x, y]: U[], ...rest)` incl. destr
//!   patterns, optional `?`, defaults, trailing comma; returns the
//!   params plus the synthesized destr let-prelude
//!
//! Bodies unchanged; the only edits are the two delegate calls and
//! the helpers' Ok(...) tails.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_fn(&mut self, is_async: bool) -> Result<Stmt, String> {
        // Span anchor (B1b) — the `function` keyword, or the `async`
        // the caller consumed one token earlier (parse_stmt's L.2 arm
        // is the only is_async caller and sits right on that contract).
        let start_pos = if is_async { self.pos - 1 } else { self.pos };
        self.pos += 1; // consume `function`
        // Phase J — `function*` generator declaration. Optional `*` token
        // sandwiched between `function` and the name marks this fn as a
        // generator; post-parse `desugar_generators` rewrites the body
        // into a state-machine class.
        let is_generator = matches!(self.peek(), Token::Star);
        if is_generator {
            self.pos += 1;
        }
        let name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!(
                    "expected function name, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        // M3 — optional type-parameter list: `function id<T, U>(...)`. If
        // present, the names are recorded on the FnDecl and the typechecker
        // treats them as placeholder types that don't resolve to a concrete
        // shape until each call site.
        let type_params = self.parse_fn_type_params()?;
        let (params, param_destr_lets) = self.parse_fn_params()?;
        let mut return_type = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            Some(self.parse_type_ann()?)
        } else {
            None
        };
        // I.2 / J.3 — record this generator so for-of and yield* can
        // look up its yield type at desugar time. The yield-type ann is
        // mandatory at desugar_generators (panics otherwise) so we
        // require it here too — without it the for-of body's `let v`
        // can't be typed.
        if is_generator {
            // P10.7 — Default-Any generator. When the user omits the
            // return-type annotation (`function* foo() {...}`), record
            // the yield type as `"any"` so for-of's body let-binding and
            // yield* delegation's intermediate vars flow through the
            // Any-tier. `ast::desugar_generators` mirrors the same
            // fallback (panic-on-None there now defaults to `"any"`).
            let raw_ann = return_type.clone().unwrap_or_else(|| "any".into());
            // V3-18 wedge — unwrap the standard wrapper-form return
            // type annotations users write for generators per TS spec
            // §3.6.4 (IterableIterator / Generator / Iterator). The
            // yield type T inside one of these wrappers is what the
            // Phase J desugar machinery needs — it builds the iterator
            // class layout from T, not from `Generator<T>`. Pre-fix
            // those wrapped anns failed at `unknown type` in check.rs.
            // Rewrite the FnDecl's return_type too so desugar_generators
            // sees the unwrapped T directly.
            let yield_ty = unwrap_generator_return_ann(&raw_ann);
            if yield_ty != raw_ann {
                return_type = Some(yield_ty.clone());
            }
            self.generator_fns.insert(name.clone(), yield_ty);
        }
        if is_async {
            if is_generator {
                // Async generators go in their own set: desugar_async
                // must NOT Promise-wrap the factory (ag() returns the
                // generator object directly per §27.6); the step
                // methods get their Promise shape via
                // desugar_generators → async_fns registration.
                self.ast.async_generator_fns.insert(name.clone());
            } else {
                self.ast.async_fns.insert(name.clone());
            }
        }
        // V3-18 wedge — TS overload signature: `function f(...): R;`
        // (no body, terminated by `;`). Type-only; the runtime impl
        // is the trailing same-named declaration. Discard by
        // returning an empty Block — the real FnDecl follows.
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
            return Ok(Stmt::Block(Vec::new()));
        }
        // body must be a block
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` (function body), got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // P-SURF S2.9 — an ordinary function has its own `this` binding,
        // so a `super()` written inside one is an early SyntaxError even
        // when the function itself sits in a derived constructor
        // (ES §15.7.1). Arrows are the exception and do not clear it.
        // Error paths do not restore, following `current_class`: a parse
        // that has failed leaves the parser in an error state where the
        // value is moot.
        let saved_super = std::mem::replace(&mut self.super_call_allowed, false);
        let saved_async_gen = std::mem::replace(&mut self.in_async_gen, is_async && is_generator);
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            body.push(self.parse_stmt()?);
        }
        self.in_async_gen = saved_async_gen;
        self.super_call_allowed = saved_super;
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => return Err(format!("expected `}}`, got {t:?} at {}", self.at())),
        }
        self.reject_lexical_shadowing_param(&params, &param_destr_lets, &body)?;
        self.reject_use_strict_with_non_simple_params(&params, &body)?;
        // V3-18 wedge — prepend per-param destructuring lets when a
        // parameter was a binding pattern. Order is preserved (lets
        // run first, then user body).
        if !param_destr_lets.is_empty() {
            // Generator decls: record how many synthesized destr lets
            // prefix the body so desugar_generators can peel them into
            // the __Gen ctor (eager param-binding timing, ES §9.2).
            if is_generator {
                self.ast
                    .gen_param_destr_prefix
                    .insert(name.clone(), param_destr_lets.len());
            }
            let mut full = param_destr_lets;
            full.extend(body);
            body = full;
        }
        Ok(Stmt::FnDecl {
            name,
            type_params,
            params,
            return_type,
            body,
            is_generator,
            span: self.span_from(start_pos),
        })
    }

    fn parse_fn_type_params(&mut self) -> Result<Vec<String>, String> {
        let mut type_params: Vec<String> = Vec::new();
        if matches!(self.peek(), Token::Lt) {
            // B2 — the whole `<T, U>` list is type-side syntax; record
            // it for the erase splice (see parse_type_ann's wrapper).
            let start_pos = self.pos;
            self.pos += 1;
            if !matches!(self.peek(), Token::Gt) {
                loop {
                    match self.peek() {
                        Token::Ident(n) => {
                            type_params.push(n.clone());
                            self.pos += 1;
                        }
                        t => {
                            return Err(format!(
                                "expected type-parameter name, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    match self.peek() {
                        Token::Comma => self.pos += 1,
                        Token::Gt => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `>` in type parameters, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
            }
            match self.peek() {
                Token::Gt => {
                    self.pos += 1;
                    let span = self.span_from(start_pos);
                    self.ast.type_ann_spans.push(span);
                }
                t => {
                    return Err(format!(
                        "expected `>` to close type parameters, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        Ok(type_params)
    }

    fn parse_fn_params(&mut self) -> Result<(Vec<Param>, Vec<Stmt>), String> {
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => return Err(format!("expected `(`, got {t:?} at {}", self.at())),
        }
        let mut params = Vec::new();
        // V3-18 wedge — function parameter destructuring pattern:
        //   function f([a, b]: number[])           — array form
        //   function f({ x, y }: { x:T, y:U })     — object form
        // Per ES spec §14.1.3 a BindingPattern (array or object) is a
        // valid FormalParameter. The lexer produces `[` / `{` where the
        // ident-name is expected; pre-fix the param parser bailed at
        // 'expected parameter name, got LBracket / LBrace'.
        //
        // Implementation: a destr pattern at the param position
        // synthesizes a fresh hidden binding name (`__param_destr_<id>`)
        // and accumulates per-element / per-field `let bound = synth[i]`
        // (or `synth.field`) into `param_destr_lets`, which is prepended
        // to the parsed body just before emitting Stmt::FnDecl.
        // Reserved-word fields go through keyword_property_name to match
        // the obj-literal / for-of-destr wedges already in tree.
        let mut param_destr_lets: Vec<Stmt> = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                let is_rest = matches!(self.peek(), Token::DotDotDot);
                if is_rest {
                    self.pos += 1;
                }
                if !is_rest && matches!(self.peek(), Token::LBracket | Token::LBrace) {
                    let synth = self.parse_destr_param(&mut param_destr_lets)?;
                    let type_ann = if matches!(self.peek(), Token::Colon) {
                        self.pos += 1;
                        Some(self.parse_type_ann()?)
                    } else {
                        None
                    };
                    // P-PARSE.6 — whole-pattern default on a destr fn
                    // param: `function f({a, b} = {a:1, b:2}) {...}`.
                    // Mirror of the arrow-fn destr default added in
                    // the same wedge.
                    let default = if matches!(self.peek(), Token::Eq) {
                        self.pos += 1;
                        Some(self.with_in_formal_params(|p| p.parse_expr())?)
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
                // V3-18 wedge — optional parameter `name?: T`. TS spec
                // §3.9.2.4: `?` permits the call site to omit the arg.
                // Subset models it as Nullable<T>. When the caller omits
                // the arg, the param receives the implicit `null` default
                // (synthesized below) — this lets `f()` Just Work for
                // `function f(x?: T)`, matching TS semantics where the
                // omitted `x` is `undefined` (subset's null sentinel).
                let optional = !is_rest && matches!(self.peek(), Token::Question);
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
                } else if is_rest {
                    // Untyped rest param is implicitly `any[]` (TS
                    // §implicit-any); synthesizing here lets every
                    // downstream face see the user-written-`any[]` shape.
                    Some("any[]".into())
                } else {
                    None
                };
                let default = if !is_rest && matches!(self.peek(), Token::Eq) {
                    self.pos += 1;
                    Some(self.with_in_formal_params(|p| p.parse_expr())?)
                } else if optional {
                    // §9.2 — see param_optional_default.
                    Some(self.implicit_optional_default(type_ann.as_deref()))
                } else {
                    None
                };
                params.push(Param {
                    name: pname,
                    type_ann,
                    default,
                    is_rest,
                });
                match self.peek() {
                    Token::Comma => {
                        if is_rest {
                            return Err(format!("rest parameter must be last at {}", self.at()));
                        }
                        self.pos += 1;
                        // V3-18 wedge — trailing comma in fn-decl param list.
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
        self.reject_duplicate_params(&params, false)?;
        Ok((params, param_destr_lets))
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::Stmt;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    /// Slice `src` by the span of the first `Stmt::FnDecl` (B1b —
    /// the fn_name_table bake hands this exact range to toString).
    fn first_fn_decl_span_text(src: &str) -> String {
        let tokens = tokenize(src).expect("tokenize");
        let ast = parse(src, &tokens).expect("parse");
        let span = ast
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::FnDecl { span, .. } => Some(*span),
                _ => None,
            })
            .expect("a FnDecl stmt");
        assert!(
            span.start != 0 || span.end != 0,
            "FnDecl span left at the (0,0) sentinel"
        );
        src[span.start as usize..span.end as usize].to_string()
    }

    #[test]
    fn fn_decl_span_covers_keyword_to_body_brace() {
        let src = "function f(a: number, b: number): number {\n  return a + b;\n}\nf(1, 2);\n";
        assert_eq!(
            first_fn_decl_span_text(src),
            "function f(a: number, b: number): number {\n  return a + b;\n}"
        );
    }

    #[test]
    fn async_fn_decl_span_includes_async_prefix() {
        let src = "async function g(x: number): Promise<number> { return x; }\n";
        assert_eq!(
            first_fn_decl_span_text(src),
            "async function g(x: number): Promise<number> { return x; }"
        );
    }

    #[test]
    fn generator_fn_decl_span_covers_star_form() {
        let src = "function* gen(): Generator<number> {\n  yield 1;\n}\n";
        assert_eq!(
            first_fn_decl_span_text(src),
            "function* gen(): Generator<number> {\n  yield 1;\n}"
        );
    }
}
