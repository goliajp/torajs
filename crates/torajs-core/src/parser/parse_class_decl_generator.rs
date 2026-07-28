//! Class generator methods — `class C { *g() { yield 1 } }`.
//!
//! P-SURF S2.1, the class half. The object-literal half
//! (`object_member_generator.rs`) is pure parser work; this one is not,
//! and three facts about the existing pipeline fix its shape:
//!
//! 1. `desugar_generators` runs **before** `desugar_classes`, so at
//!    generator time a class is still a `ClassDecl` and there is no
//!    `__cm_<C>__<m>` FnDecl yet to mark `is_generator`.
//! 2. The generator desugar rewrites a `function*` into a `__Gen_<name>`
//!    **class** whose fields hold the generator's parameters — that is
//!    how state survives from one `next()` to the next.
//! 3. It has no env channel at all: `hoist_gen_fn_exprs` documents that
//!    prep rewrites params and lifted lets to `this.<name>` and
//!    everything else must resolve as a module-level global.
//!
//! (2) and (3) together mean the receiver can only reach the body as a
//! **parameter**. And because prep reaches its own fields through
//! `this`, an `Expr::This` left standing until `desugar_classes` would
//! be read as the `__Gen_*` instance rather than the class instance —
//! the wrong receiver, silently. So the body's `this` is minted as
//! `Ident("__genrecv")` while parsing it, via
//! `Parser::in_gen_class_method`.
//!
//! **Why not `__this`.** That was the first attempt and it fails
//! loudly: `__this` is load-bearing in the class-method world — it is
//! the first-parameter name every `__cm_*` method carries. The
//! generator desugar turns this parameter into a field of the
//! `__Gen_*` class, so a field named `__this` collides with the
//! `__this` parameter that `__Gen_*`'s own `next()` receives once
//! `desugar_classes` reaches it: `redeclaration of __this in current
//! scope`. Same shape as the `__new_` collision in rotation 225 —
//! borrowing a load-bearing name inherits everything it means. A
//! private name keeps the two apart.
//!
//! What this emits, for `class C { *g(a) { yield this.x + a } }`:
//!
//! ```text
//! function* __cm_gen_C__g(__genrecv: any, a) { yield __genrecv.x + a }  // synth
//! class C { g(a) { return __cm_gen_C__g(this, a); } }             // ordinary forwarder
//! ```
//!
//! The forwarder is an ordinary method, so vtable construction,
//! `method_owners`, visibility and every other class mechanism stays
//! untouched — `c.g()` dispatches exactly as it did before. Its own
//! `this` is a plain `Expr::This`, rewritten by `desugar_classes` in the
//! normal way, because the forwarder is not itself a generator body.
//!
//! `static *g() {}` rides the same path; the forwarder simply lands in
//! `static_methods`, and its `this` is the class object the same as any
//! other static method's.

use super::Parser;
use crate::ast::{ClassMethod, Expr, GEN_METHOD_PREFIX, GEN_RECV_PARAM, Param, Stmt, Visibility};
use crate::lexer::Token;

impl<'a> Parser<'a> {
    /// Parse `*<name>(params) { body }` as a class member. The caller
    /// has already consumed any modifier prefix and verified the current
    /// token is `Star`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_class_generator_method(
        &mut self,
        class_name: &str,
        parent: Option<&str>,
        is_static: bool,
        is_async: bool,
        visibility: Visibility,
        member_span_start: u32,
        methods: &mut Vec<ClassMethod>,
        static_methods: &mut Vec<ClassMethod>,
    ) -> Result<(), String> {
        // Consume `*`.
        self.pos += 1;

        let mut visibility = visibility;
        let member_name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t if Self::keyword_property_name(t).is_some() => {
                Self::keyword_property_name(t).unwrap().to_string()
            }
            // P-SURF S2.2 × S2.1 — `*#g() {}`. Mangled and forced
            // Private exactly as the ordinary-member path does
            // (`parse_class_decl_member.rs`), so everything downstream
            // sees a regular name and `this.#g()` resolves to it via the
            // same rewrite. `static *#g()` (S2.37) rides the same
            // mangle: the hoisted `function*` takes the class object as
            // its `any`-typed receiver exactly as a public `static *g()`
            // does, so nothing downstream needs a static-private lane.
            Token::PrivateIdent(n) => {
                visibility = Visibility::Private;
                format!("__priv_{class_name}__{n}")
            }
            t => {
                return Err(format!(
                    "expected generator method name after `*` in class `{class_name}`, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;

        let (mut params, destr_lets) = self.parse_param_list()?;
        self.infer_default_param_anns(&mut params);
        // A method definition takes UniqueFormalParameters (§15.4).
        self.reject_duplicate_params(&params, true)?;

        let return_type = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            let ann = self.parse_type_ann()?;
            Some(super::type_ann_helpers::unwrap_generator_return_ann(&ann))
        } else {
            None
        };

        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` after generator method `{member_name}` header in class `{class_name}`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // The body's `this` mints `Ident(GEN_RECV_PARAM)` from here — see
        // the module docs for why it cannot wait for `desugar_classes`.
        // Every expression the body mints lands in the arena from here
        // on, so remembering the high-water mark gives an exact range to
        // sweep for `super` afterwards without walking statements.
        let body_expr_start = self.ast.exprs.len();
        let saved_in_gen = self.in_gen_class_method;
        self.in_gen_class_method = true;
        // S2.9 — a generator method is a method: `super()` in it is an
        // early SyntaxError (ES §15.7.1). `super.m()` stays legal and is
        // resolved below.
        let saved_super = std::mem::replace(&mut self.super_call_allowed, false);
        let saved_async_gen = std::mem::replace(&mut self.in_async_gen, is_async);
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            match self.parse_stmt() {
                Ok(s) => body.push(s),
                Err(e) => {
                    self.in_gen_class_method = saved_in_gen;
                    self.in_async_gen = saved_async_gen;
                    return Err(e);
                }
            }
        }
        self.in_gen_class_method = saved_in_gen;
        self.in_async_gen = saved_async_gen;
        self.super_call_allowed = saved_super;
        // Byte end of the MethodDefinition span (ES §20.2.3.5) — the
        // closing `}`, captured before it is consumed, same as the
        // ordinary method path.
        let member_span_end = self.tokens.get(self.pos).map(|t| t.span.end).unwrap_or(0);
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end generator method `{member_name}` body in class `{class_name}`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let span = crate::lexer::Span {
            start: member_span_start,
            end: member_span_end,
        };
        // A destructured parameter (`*g({a, b}) {}`) becomes a synthetic
        // `__param_destr_N` param plus a prefix of `let` statements that
        // unpack it. `desugar_generators` peels exactly those into the
        // `__Gen_*` constructor — ES §9.2 binds parameters eagerly, so a
        // throwing destructure fires at the call, not at the first
        // `next()` — and it learns the count from this table. The count is
        // of body statements, so the receiver parameter prepended below
        // does not enter into it.
        self.reject_lexical_shadowing_param(&params, &destr_lets, &body)?;
        let destr_prefix = destr_lets.len();
        let body = if destr_lets.is_empty() {
            body
        } else {
            let mut full = destr_lets;
            full.extend(body);
            full
        };

        Self::rewrite_supercalls_in_range(&mut self.ast, body_expr_start, parent);

        // The hoisted generator: receiver first, then the user's params.
        // An instance receiver carries the declaring class's own name
        // (S2.11): `super.m()` rewrites below to `__cm_<Parent>__<m>`,
        // whose first parameter is the parent's nominal type, and `any`
        // is not admitted into a heap-typed parameter slot. Naming the
        // declaring class rather than the parent is what makes the
        // inherited direction work too — the receiver slot admits a
        // subclass by prefix layout, so a grandchild instance reaching a
        // generator declared two levels up still type-checks.
        //
        // `static *g()` keeps `any`: there the receiver is the class
        // object rather than an instance, and no nominal type describes
        // it.
        let synth_name = format!("{GEN_METHOD_PREFIX}{class_name}__{member_name}");
        if destr_prefix > 0 {
            self.ast
                .gen_param_destr_prefix
                .insert(synth_name.clone(), destr_prefix);
        }
        let mut synth_params = vec![Param {
            name: GEN_RECV_PARAM.into(),
            type_ann: Some(if is_static {
                "any".into()
            } else {
                class_name.to_string()
            }),
            default: None,
            is_rest: false,
        }];
        synth_params.extend(params.iter().cloned());
        // P-SURF S2.18 — `async *g() {}`. Async-ness rides a side table
        // keyed by the declared name, so the hoisted `function*` is
        // registered exactly as a top-level `async function*` would be.
        // The set is `async_generator_fns` rather than `async_fns`
        // because §27.6 says the factory hands back the generator object
        // directly; Promise-wrapping it is `desugar_async`'s job for
        // ordinary async functions only, and the step methods get their
        // Promise shape later via `desugar_generators`.
        //
        // The forwarder below stays an ordinary method for the same
        // reason: it returns that object unchanged.
        if is_async {
            self.ast.async_generator_fns.insert(synth_name.clone());
        }
        self.synth_classes.push(Stmt::FnDecl {
            name: synth_name.clone(),
            type_params: Vec::new(),
            params: synth_params,
            return_type,
            body,
            is_generator: true,
            span,
        });

        // The forwarder: `return __cm_gen_C__g(this, ...params)`.
        let callee = self.ast.add_expr(Expr::Ident(synth_name));
        let mut args = vec![self.ast.add_expr(Expr::This)];
        for p in &params {
            args.push(self.ast.add_expr(Expr::Ident(p.name.clone())));
        }
        let call = self.ast.add_expr(Expr::Call { callee, args });
        let forwarder = ClassMethod {
            name: member_name,
            params,
            // NOT `return_type` — that is the unwrapped YIELD type,
            // which is what the hoisted `function*` above wants (the
            // desugar builds the iterator class from T and rewrites
            // the decl to answer that class). The forwarder answers
            // the generator OBJECT, so carrying T here declared
            // `*g(): Generator<number>` to return a number while its
            // body returned the class — a type error on every
            // annotated generator method, sync and async alike.
            // Leaving it open lets the body's own verdict stand, the
            // same way an unannotated one already worked.
            return_type: None,
            body: vec![Stmt::Return(Some(call))],
            is_abstract: false,
            visibility,
            accessor_kind: None,
            span,
        };
        if is_static {
            static_methods.push(forwarder);
        } else {
            methods.push(forwarder);
        }
        Ok(())
    }

    /// Resolve `super.m(args)` markers minted inside a hoisted generator
    /// body. `from` is the arena high-water mark taken before the body
    /// was parsed, so the range covers exactly that body's expressions.
    ///
    /// This has to run in the parser rather than in `desugar_classes`,
    /// which normally owns the rewrite: by the time that pass runs,
    /// `desugar_generators` has already moved the body again — into the
    /// `__Gen_*` state machine — and it is no longer reachable as a
    /// method body of any class.
    ///
    /// The receiver is the synthesized parameter rather than `this`, for
    /// the same reason the body's `this` was: `this` inside the state
    /// machine denotes the `__Gen_*` instance.
    ///
    /// With no parent the markers are left alone, so the existing "no
    /// parent class" diagnostic still fires.
    ///
    /// The receiver's type is what made the call type-check: it is the
    /// declaring class's own name rather than `any`, so it meets
    /// `__cm_<Parent>__<m>`'s nominal first parameter (S2.11 — see the
    /// receiver-parameter comment above for why the declaring class and
    /// not the parent).
    ///
    /// **Known gap.** Only the *direct* parent is consulted, so
    /// `class C extends B extends A` reaching A's `m` rewrites to
    /// `__cm_B__m` and fails as an unknown identifier. That is not a
    /// generator defect — `desugar_classes_super` resolves ordinary
    /// methods the same way and the plain-method spelling fails
    /// identically (measured). Walking the chain fixes both at once.
    fn rewrite_supercalls_in_range(ast: &mut crate::ast::Ast, from: usize, parent: Option<&str>) {
        let Some(parent_name) = parent else {
            return;
        };
        for i in from..ast.exprs.len() {
            let (m_name, args) = match &ast.exprs[i] {
                Expr::Call { callee, args } => match &ast.exprs[callee.0 as usize] {
                    Expr::Ident(n) => match n.strip_prefix("__supercall__") {
                        Some(m) => (m.to_string(), args.clone()),
                        None => continue,
                    },
                    _ => continue,
                },
                _ => continue,
            };
            let callee = ast.add_expr(Expr::Ident(format!("__cm_{parent_name}__{m_name}")));
            let recv = ast.add_expr(Expr::Ident(GEN_RECV_PARAM.into()));
            let mut new_args = Vec::with_capacity(args.len() + 1);
            new_args.push(recv);
            new_args.extend(args);
            ast.exprs[i] = Expr::Call {
                callee,
                args: new_args,
            };
        }
    }
}
