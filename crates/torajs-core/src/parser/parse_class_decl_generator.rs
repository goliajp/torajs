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
use crate::ast::{ClassMethod, Visibility};
use crate::lexer::Token;

impl<'a> Parser<'a> {
    /// The mangled member name for `*#g()`, and the two §15.7.1 /
    /// §15.7 duties that come with a private ClassElementName:
    /// `#constructor` is a Syntax Error in every position, and the
    /// RAW name registers in the enclosing class's private scope so a
    /// nested `#g` reference binds lexically. Both are verbatim what
    /// the ordinary-member path in `parse_class_decl_member.rs` does,
    /// so `static *#g()` (S2.37) needs no static-private lane: the
    /// hoisted `function*` takes the class object as its `any`-typed
    /// receiver exactly as a public `static *g()` does.
    fn generator_private_member_name(
        &mut self,
        n: &str,
        class_name: &str,
    ) -> Result<String, String> {
        if n == "constructor" {
            return Err(format!(
                "class member may not be named `#constructor` in class `{class_name}` at {} (ES §15.7.1)",
                self.at()
            ));
        }
        if let Some(&sid) = self.class_stack.last() {
            self.ast.class_private_scopes[sid as usize]
                .1
                .insert(n.to_string());
        }
        Ok(format!("__priv_{class_name}__{n}"))
    }

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
            // ES §12.7.2 — escaped ReservedWord as an IdentifierName.
            Token::EscapedIdent(n) => n.clone(),
            t if Self::keyword_property_name(t).is_some() => {
                Self::keyword_property_name(t).unwrap().to_string()
            }
            // P-SURF S2.2 × S2.1 — `*#g() {}`; see the helper above.
            Token::PrivateIdent(n) => {
                let n = n.clone();
                visibility = Visibility::Private;
                self.generator_private_member_name(&n, class_name)?
            }
            t => {
                return Err(format!(
                    "expected generator method name after `*` in class `{class_name}`, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        self.finish_class_generator_method(
            class_name,
            parent,
            is_static,
            is_async,
            visibility,
            member_span_start,
            member_name,
            methods,
            static_methods,
        )
    }
}
