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
use crate::ast::{ClassMethod, Expr, Param, Stmt, Visibility};
use crate::lexer::Token;

/// Parameter name carrying the receiver into a hoisted class generator
/// method. Deliberately not `__this` — see the module docs.
pub(super) const GEN_RECV_PARAM: &str = "__genrecv";

impl<'a> Parser<'a> {
    /// Parse `*<name>(params) { body }` as a class member. The caller
    /// has already consumed any modifier prefix and verified the current
    /// token is `Star`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_class_generator_method(
        &mut self,
        class_name: &str,
        is_static: bool,
        visibility: Visibility,
        member_span_start: u32,
        methods: &mut Vec<ClassMethod>,
        static_methods: &mut Vec<ClassMethod>,
    ) -> Result<(), String> {
        // Consume `*`.
        self.pos += 1;

        let member_name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t if Self::keyword_property_name(t).is_some() => {
                Self::keyword_property_name(t).unwrap().to_string()
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
        let saved_in_gen = self.in_gen_class_method;
        self.in_gen_class_method = true;
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            match self.parse_stmt() {
                Ok(s) => body.push(s),
                Err(e) => {
                    self.in_gen_class_method = saved_in_gen;
                    return Err(e);
                }
            }
        }
        self.in_gen_class_method = saved_in_gen;
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
        let body = if destr_lets.is_empty() {
            body
        } else {
            let mut full = destr_lets;
            full.extend(body);
            full
        };

        // The hoisted generator: receiver first, then the user's params.
        // `any` rather than the class's nominal type — the generator
        // desugar turns this parameter into a `__Gen_*` field, and a
        // nominal annotation there would demand a layout the state
        // machine class does not have.
        let synth_name = format!("__cm_gen_{class_name}__{member_name}");
        let mut synth_params = vec![Param {
            name: GEN_RECV_PARAM.into(),
            type_ann: Some("any".into()),
            default: None,
            is_rest: false,
        }];
        synth_params.extend(params.iter().cloned());
        self.synth_classes.push(Stmt::FnDecl {
            name: synth_name.clone(),
            type_params: Vec::new(),
            params: synth_params,
            return_type: return_type.clone(),
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
            return_type,
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
}
