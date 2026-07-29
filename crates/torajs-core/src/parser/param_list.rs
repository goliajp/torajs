//! Parameter-list cluster (chunk 416).
//!
//! Extracted verbatim from parser.rs — the two shared param-list
//! parsers used by class methods / ctors and function-expression
//! forms:
//! - parse_ctor_param_list — ctor list with TS parameter-property
//!   shorthand (`constructor(public x: number, ...)`), returning the
//!   promoted-field side-table + destr-let prelude
//! - parse_param_list — plain `(p1: T, p2: T, ...)` list with
//!   destr-pattern / optional-`?` / default / rest wedges, returning
//!   the destr-let prelude for the caller to prepend
//!
//! Callers live in parse_class_member_method / object_member /
//! object_literal / fn_expr siblings. Both promoted `pub(super)`
//! per the sibling-impl pack pattern. Body unchanged.

use super::*;

impl<'a> Parser<'a> {
    /// A duplicate parameter name is a SyntaxError, and *where* decides
    /// whether unconditionally:
    ///
    /// - A **method definition** (§15.4) and an **arrow** (§15.3.1) take
    ///   `UniqueFormalParameters`, so duplicates are refused whatever
    ///   the list looks like. `unique = true`.
    /// - A **function declaration or expression** takes plain
    ///   `FormalParameters`, whose §15.1.1 early error applies only in
    ///   strict code or when the list is not simple. In sloppy code with
    ///   a simple list `function f(a, a) {}` is legal, and test262
    ///   asserts it: `param-duplicated-non-strict.js` (×2) and
    ///   `S10.2.1_A2.js` run it and expect it to work. Refusing there
    ///   cost exactly those three cases — measured, not predicted.
    ///
    /// Class bodies are always strict (§11.2.2), so a `function` nested
    /// in one is refused too. `current_class` is an exact test for that
    /// rather than a heuristic: everything lexically inside a class body
    /// is strict code.
    ///
    /// P-SURF S2.9 — tr used to refuse `*m(x = 0, x)` by accident. The
    /// generator desugar turns parameters into fields of the `__Gen_*`
    /// state-machine class, two same-named parameters became two
    /// same-named fields, and the field-conflict check panicked. That
    /// refusal was right for the wrong reason and said so out loud,
    /// naming a synthesized class the user never wrote; the plain
    /// spelling `function f(x = 0, x) {}` was not refused at all.
    ///
    /// Called from every parameter-list parser rather than from one
    /// shared place, because there is no shared place: `parse_fn` and
    /// the arrow parser each carry their own copy of the loop.
    pub(super) fn reject_duplicate_params(
        &self,
        params: &[Param],
        unique: bool,
    ) -> Result<(), String> {
        // §15.1.2 IsSimpleParameterList — no default, no rest, no
        // binding pattern (which arrives as a synthesized holder name).
        let simple = params
            .iter()
            .all(|p| p.default.is_none() && !p.is_rest && !p.name.starts_with("__param_destr_"));
        if !unique && simple && self.current_class.is_none() {
            return Ok(());
        }
        for (i, p) in params.iter().enumerate() {
            if params[..i].iter().any(|q| q.name == p.name) {
                return Err(format!(
                    "duplicate parameter name `{}` at {}",
                    p.name,
                    self.at()
                ));
            }
        }
        Ok(())
    }

    /// ES §14.2.1 early error: a `let` / `const` at the top level of a
    /// function body may not repeat a parameter name. (`var` may — it
    /// is the same variable, per §14.3.2.1 — and so may a nested
    /// function declaration, which is var-scoped.)
    ///
    /// P-SURF S2.9, the other half of [`Self::reject_duplicate_params`]
    /// and refused by the same accident until now: the name collided as
    /// two fields of the generator's `__Gen_*` class, which caught
    /// `*foo(a) { let a = 3 }` and missed the plain spelling.
    ///
    /// `destr_lets` carries the bindings a destructuring parameter
    /// unpacks (`function f({ a }) {}` binds `a`), which are parameter
    /// names for this purpose even though they arrive as statements.
    /// The synthesized `__param_destr_N` holders are not — they are
    /// unspellable, so no user declaration can collide with one.
    pub(super) fn reject_lexical_shadowing_param(
        &self,
        params: &[Param],
        destr_lets: &[Stmt],
        body: &[Stmt],
    ) -> Result<(), String> {
        let mut bound: Vec<&str> = params
            .iter()
            .map(|p| p.name.as_str())
            .filter(|n| !n.starts_with("__param_destr_"))
            .collect();
        for s in destr_lets {
            if let Stmt::LetDecl { name, .. } = s {
                bound.push(name.as_str());
            }
        }
        for s in body {
            let Stmt::LetDecl {
                name,
                is_var: false,
                ..
            } = s
            else {
                continue;
            };
            if bound.contains(&name.as_str()) {
                return Err(format!(
                    "`{name}` is declared in the body and is already a parameter, at {}",
                    self.at()
                ));
            }
        }
        Ok(())
    }

    /// V3-18 wedge — TS parameter-property shorthand
    /// (`constructor(public x: number, private readonly y: string)`).
    /// Returns the regular param list plus a side-table of
    /// (param_index, visibility, is_readonly) entries for params that
    /// should be promoted to instance fields, plus the destr-let vec
    /// for any binding-pattern params (synthesized `__param_destr_<id>`
    /// hidden bindings + per-element / per-field lets to prepend to the
    /// ctor body — caller does the prepend before promoted-prop assigns).
    /// Visibility / readonly modifiers can't combine with a destr
    /// pattern at the same param position (a binding pattern has no
    /// single field-name to promote).
    pub(super) fn parse_ctor_param_list(
        &mut self,
    ) -> Result<(Vec<Param>, Vec<(usize, ast::Visibility, bool)>, Vec<Stmt>), String> {
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => return Err(format!("expected `(`, got {t:?} at {}", self.at())),
        }
        let mut params = Vec::new();
        let mut promoted: Vec<(usize, ast::Visibility, bool)> = Vec::new();
        let mut destr_lets: Vec<Stmt> = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                // Consume any TS modifiers: visibility (`public` /
                // `private` / `protected`) and `readonly`. Order is
                // visibility-then-readonly per TS, but we accept any
                // combination once.
                let mut vis: Option<ast::Visibility> = None;
                let mut rd = false;
                loop {
                    let Token::Ident(s) = self.peek() else { break };
                    match s.as_str() {
                        "public" => {
                            if vis.is_some() {
                                return Err(format!(
                                    "duplicate visibility modifier in ctor param at {}",
                                    self.at()
                                ));
                            }
                            vis = Some(ast::Visibility::Public);
                            self.pos += 1;
                        }
                        "private" => {
                            if vis.is_some() {
                                return Err(format!(
                                    "duplicate visibility modifier in ctor param at {}",
                                    self.at()
                                ));
                            }
                            vis = Some(ast::Visibility::Private);
                            self.pos += 1;
                        }
                        "protected" => {
                            if vis.is_some() {
                                return Err(format!(
                                    "duplicate visibility modifier in ctor param at {}",
                                    self.at()
                                ));
                            }
                            vis = Some(ast::Visibility::Protected);
                            self.pos += 1;
                        }
                        "readonly" => {
                            if rd {
                                return Err(format!(
                                    "duplicate `readonly` in ctor param at {}",
                                    self.at()
                                ));
                            }
                            rd = true;
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
                let is_rest = matches!(self.peek(), Token::DotDotDot);
                if is_rest {
                    self.pos += 1;
                }
                if !is_rest && matches!(self.peek(), Token::LBracket | Token::LBrace) {
                    if vis.is_some() || rd {
                        return Err(format!(
                            "ctor destructuring param can't carry visibility / readonly modifiers at {}",
                            self.at()
                        ));
                    }
                    let synth = self.parse_destr_param(&mut destr_lets)?;
                    let type_ann = if matches!(self.peek(), Token::Colon) {
                        self.pos += 1;
                        Some(self.parse_type_ann()?)
                    } else {
                        None
                    };
                    params.push(Param {
                        name: synth,
                        type_ann,
                        default: None,
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
                                "expected `,` or `)` after destr ctor param, got {t:?} at {}",
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
                } else {
                    None
                };
                let default = if !is_rest && matches!(self.peek(), Token::Eq) {
                    self.pos += 1;
                    Some(self.parse_expr()?)
                } else if optional {
                    // §9.2 — an absent optional binds undefined; an
                    // `any` / un-annotated slot carries the real
                    // undefined box (`typeof b === "undefined"` now
                    // answers per spec). A TYPED optional keeps the
                    // legacy implicit null — its __nullable(T) slot
                    // has no undefined arm (registered).
                    let undef_ok = type_ann
                        .as_deref()
                        .is_none_or(|a| a == "__nullable(any)" || a == "any");
                    Some(if undef_ok {
                        self.ast.add_expr(Expr::Ident("undefined".into()))
                    } else {
                        self.ast.add_expr(Expr::Null)
                    })
                } else {
                    None
                };
                let idx = params.len();
                params.push(Param {
                    name: pname,
                    type_ann,
                    default,
                    is_rest,
                });
                if vis.is_some() || rd {
                    promoted.push((idx, vis.unwrap_or(ast::Visibility::Public), rd));
                }
                match self.peek() {
                    Token::Comma => {
                        if is_rest {
                            return Err(format!("rest parameter must be last at {}", self.at()));
                        }
                        self.pos += 1;
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    }
                    Token::RParen => break,
                    t => {
                        return Err(format!(
                            "expected `,` or `)` in params, got {t:?} at {}",
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
        self.reject_duplicate_params(&params, true)?;
        Ok((params, promoted, destr_lets))
    }

    /// Shared helper: parse a `(p1: T, p2: T, ...)` parameter list.
    /// Used by class methods/ctors. (Existing `parse_fn` / `parse_arrow_fn`
    /// have their own copies inlined; not refactoring them here to keep the
    /// M5.1 diff focused.)
    /// V3-18 wedge — return `(params, destr_lets)`. Destr_lets is the
    /// vec of `let bound = synth.field` (or `synth[i]`) statements
    /// generated when one or more params are binding patterns rather
    /// than identifiers. The caller is responsible for prepending
    /// destr_lets to the parsed body. When no destr params appear,
    /// destr_lets is empty and the caller's prepend is a no-op.
    pub(super) fn parse_param_list(&mut self) -> Result<(Vec<Param>, Vec<Stmt>), String> {
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => return Err(format!("expected `(`, got {t:?} at {}", self.at())),
        }
        let mut params = Vec::new();
        let mut param_destr_lets: Vec<Stmt> = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                // Rest parameter: `...name`. Must be the last param;
                // the post-loop check enforces it.
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
                    // P-PARSE.6 — whole-pattern default on a destr
                    // method param: third call site for the same
                    // destr-default plumbing (parse_fn / parse_arrow_fn
                    // / class-method parse_param_list).
                    let default = if matches!(self.peek(), Token::Eq) {
                        self.pos += 1;
                        Some(self.parse_expr()?)
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
                // V3-18 wedge — optional parameter `name?: T`. Mirrors
                // the parse_fn version so class methods accept the
                // same shape.
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
                } else {
                    None
                };
                // Default value: `= <expr>`. Evaluated at the call
                // site (not in callee scope) when the caller omits
                // the arg. Not allowed on rest params. Optional `name?: T`
                // without an explicit default binds per §9.2:
                // undefined for an `any` / un-annotated slot (the
                // real box — `typeof y === "undefined"` answers per
                // spec), the legacy implicit null for a TYPED
                // optional (its __nullable(T) slot has no undefined
                // arm — registered). Matches parse_fn / the ctor
                // list above.
                let default = if !is_rest && matches!(self.peek(), Token::Eq) {
                    self.pos += 1;
                    Some(self.parse_expr()?)
                } else if optional {
                    let undef_ok = type_ann
                        .as_deref()
                        .is_none_or(|a| a == "__nullable(any)" || a == "any");
                    Some(if undef_ok {
                        self.ast.add_expr(Expr::Ident("undefined".into()))
                    } else {
                        self.ast.add_expr(Expr::Null)
                    })
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
                        // V3-18 wedge — trailing comma in param list,
                        // per JS spec §13.3.3 ('function f(a, b,)'). Detect
                        // immediately-following ')' and break out.
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    }
                    Token::RParen => break,
                    t => {
                        return Err(format!(
                            "expected `,` or `)` in params, got {t:?} at {}",
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
        Ok((params, param_destr_lets))
    }

    /// RFC 20260714-t262-top-clusters 刀 1b — an un-annotated method
    /// param with a default has no type source: the fn / arrow tiers
    /// get implicit-generic TypeVar + call-site mono instantiation,
    /// but method positions (class `__cm_` / obj-literal field
    /// closure) dispatch without an instantiation site, so the
    /// TypeVar ABI reads garbage (SIGSEGV when the body uses the
    /// param) or the checker rejects ("requires a type annotation").
    /// TS-spec posture: the param's type IS the default's type —
    /// infer from the literal shape; a non-literal default falls to
    /// `any` (the Any tier boxes correctly through every dispatch
    /// path). Destr synth params arrive already any-forced (blade 5)
    /// with `type_ann` set, so they pass through untouched; the `x?`
    /// implicit-null default keeps today's posture.
    pub(super) fn infer_default_param_anns(&self, params: &mut [Param]) {
        for p in params.iter_mut() {
            if p.type_ann.is_some() || p.is_rest {
                continue;
            }
            let Some(d) = p.default else { continue };
            let ann = match self.ast.get_expr(d) {
                Expr::Number(_) => "number",
                Expr::String(_) => "string",
                Expr::Bool(_) => "boolean",
                Expr::Null => continue,
                _ => "any",
            };
            p.type_ann = Some(ann.to_string());
        }
    }
}
