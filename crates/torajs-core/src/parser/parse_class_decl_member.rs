//! Class-decl member helpers — the static block and the member name.
//! The field arms moved to `parse_class_decl_field` (rotation 618)
//! along the seam this doc already named: everything there answers
//! "what does a FIELD declaration turn into". Originally split from
//! `parse_class_decl.rs::parse_class_decl_with_abstract` (2026-07-03,
//! fn-debt decomp). Bodies verbatim; mechanical rewrites:
//! `name.clone()` → `name.to_string()` (`&str` param, same as the
//! chunk-174/175 method/field siblings), `explicit_visibility = ..`
//! → `*explicit_visibility = ..` (`&mut` param), tail returns
//! adjusted.

use super::*;

impl<'a> Parser<'a> {
    /// `static { stmts }` class static block (P8.3-A2, ES2022
    /// §15.7.10). Caller detected `static` + `{` and this consumes
    /// both plus the block body, pushing StaticInit::Block.
    pub(super) fn parse_class_static_block(
        &mut self,
        name: &str,
        static_init: &mut Vec<StaticInit>,
    ) -> Result<(), String> {
        self.pos += 2; // consume `static` + `{`
        let expr_mark = self.ast.exprs.len() as u32;
        // S2.9 — a static block is not a constructor, so `super()` in it
        // is an early SyntaxError (ES §15.7.1). §15.7.1 also forbids
        // `await` anywhere in a ClassStaticBlockBody.
        let saved_super = std::mem::replace(&mut self.super_call_allowed, false);
        let saved_await = std::mem::replace(&mut self.await_allowed, false);
        // §15.7.1 — the ONE place the two bits part company in the
        // other direction: a static block may not contain an
        // AwaitExpression (so `await_allowed` is false) AND may not
        // spell `await` as a name (ContainsAwait covers both), so the
        // grammar parameter stays ON. A nested non-async function
        // inside the block turns it back off, which is what makes
        // test262's `static { (() => { class await {} }) }` legal.
        let saved_await_res = std::mem::replace(&mut self.await_reserved, true);
        // 420-03 — §15.7.14 binds `this` to the class object in a
        // static block, the same binding a static method body gets.
        // Only the method arm used to say so, so a block's `this`
        // reached `desugar_classes` pass 2 unregistered and became the
        // instance receiver `__this`: `static { this.x = 1 }` threw
        // where it should have written an own property on the class.
        let saved_static_this =
            std::mem::replace(&mut self.static_this_class, Some(name.to_string()));
        let mut block_stmts: Vec<Stmt> = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let s = match self.parse_stmt() {
                Ok(s) => s,
                Err(e) => {
                    self.static_this_class = saved_static_this;
                    self.await_allowed = saved_await;
                    self.await_reserved = saved_await_res;
                    self.super_call_allowed = saved_super;
                    return Err(e);
                }
            };
            block_stmts.push(s);
        }
        self.static_this_class = saved_static_this;
        self.await_allowed = saved_await;
        self.await_reserved = saved_await_res;
        self.super_call_allowed = saved_super;
        if !matches!(self.peek(), Token::RBrace) {
            return Err(format!(
                "expected `}}` to close static-block in class `{name}` at {}",
                self.at()
            ));
        }
        self.pos += 1; // consume `}`
        // §15.7.1 — the second of the two productions this early error
        // is stated for (see the walk's own doc).
        if super::class_field_early_errors::static_block_contains_arguments(&self.ast, &block_stmts)
        {
            return Err(format!(
                "`arguments` is not allowed in a class static block in class \
                 `{name}` at {} (ES §15.7.1)",
                self.at()
            ));
        }
        self.ast.class_static_init_expr_ranges.push((
            name.to_string(),
            static_init.len(),
            expr_mark,
            self.ast.exprs.len() as u32,
        ));
        static_init.push(StaticInit::Block(block_stmts));
        Ok(())
    }

    /// Class member name: computed `[Symbol.x]` key (P5.2),
    /// `#name` PrivateIdentifier (P8.1, mangled + forced Private),
    /// reserved-word member names (V3-18), or plain ident. Returns
    /// `(member_name, consumed_computed_name)`.
    pub(super) fn parse_class_member_name(
        &mut self,
        name: &str,
        _is_static: bool,
        explicit_visibility: &mut Option<ast::Visibility>,
    ) -> Result<(PropKey, bool), String> {
        // P5.2 + RFC 20260802-class-computed-member 刀 1 — computed
        // class member key `[<key>]`. Three compile-time-foldable
        // shapes install under a static name:
        //   * `[Symbol.<chain>]` → `__sym_Symbol_<chain>__` (the
        //     vtable / iterator-protocol consumers key off exactly
        //     this synthetic name),
        //   * `["str"]` whole-literal key → the string itself,
        //   * `[0x10]` whole-literal numeric key → its §6.1.6.1.20
        //     canonical string ("16"), same fold as object literals.
        // Every other expression is a RUNTIME computed key
        // (§15.4 ClassElementName → ComputedPropertyName evaluates at
        // class-definition time); the runtime install lane is 刀 2 of
        // the RFC, so those reject loudly here. The old behaviour —
        // folding ANY ident chain to `__sym_<chain>__` — silently
        // installed `[k]()` under a name unrelated to the runtime
        // value of `k`, which is exactly the silent-wrong shape the
        // reject replaces.
        // Body parsing falls through to the normal method branch
        // by emitting a synthetic name + advancing past `]`.
        let mut consumed_computed_name = false;
        let member_name = if matches!(self.peek(), Token::LBracket) {
            self.pos += 1;
            let key = match self.peek() {
                // RFC 20260809 knife 4 — ONLY `Symbol.iterator` keeps
                // the legacy `__sym_<chain>__` name fold: that
                // encoding is what the iterator-protocol dispatch
                // (`__cm_<C>____sym_Symbol_iterator__`, four lowering
                // sites + the generator-class synth) resolves by
                // NAME. Every other `Symbol.<x>` member key
                // (dispose / asyncDispose / toPrimitive / …) falls
                // through to the `_ =>` runtime-computed arm below,
                // whose class-decl-position reify defines the member
                // under the real Symbol cell — the fold gave those a
                // fake string name no reader could ever reach
                // (`c[Symbol.dispose]` answered undefined).
                Token::Ident(head)
                    if head == "Symbol"
                        && matches!(
                            self.tokens.get(self.pos + 1).map(|t| &t.token),
                            Some(Token::Dot)
                        )
                        && matches!(
                            self.tokens.get(self.pos + 2).map(|t| &t.token),
                            Some(Token::Ident(n)) if n == "iterator"
                        )
                        && matches!(
                            self.tokens.get(self.pos + 3).map(|t| &t.token),
                            Some(Token::RBracket)
                        ) =>
                {
                    self.pos += 3; // `Symbol` `.` `iterator`
                    PropKey::from("__sym_Symbol_iterator__")
                }
                Token::String(s)
                    if matches!(
                        self.tokens.get(self.pos + 1).map(|t| &t.token),
                        Some(Token::RBracket)
                    ) =>
                {
                    let k = PropKey::from(s.clone());
                    self.pos += 1;
                    k
                }
                Token::Number(n)
                    if matches!(
                        self.tokens.get(self.pos + 1).map(|t| &t.token),
                        Some(Token::RBracket)
                    ) =>
                {
                    let k = crate::ast::number_prop_key(*n);
                    self.pos += 1;
                    k
                }
                _ => {
                    // RFC 20260802 刀 2 — every other expression is a
                    // RUNTIME computed key (§15.4 ClassElementName →
                    // ComputedPropertyName evaluates at class-
                    // definition time). The member installs under a
                    // unique `__ccm_<n>__` sentinel through the
                    // ordinary method / accessor machinery;
                    // desugar_classes emits the ToPropertyKey +
                    // define patch at the class-decl position.
                    let key_expr = self.parse_assign()?;
                    let sentinel = format!("__ccm_{}__", self.ast.class_computed_keys.len());
                    self.ast
                        .class_computed_keys
                        .insert((name.to_string(), sentinel.clone()), key_expr);
                    PropKey::from(sentinel)
                }
            };
            if !matches!(self.peek(), Token::RBracket) {
                let t = self.peek().clone();
                return Err(format!(
                    "expected `]` to close computed class member key for `{name}`, got {t:?} at {}",
                    self.at()
                ));
            }
            self.pos += 1; // consume `]`
            consumed_computed_name = true;
            key
        } else {
            match self.peek() {
                Token::Ident(n) => PropKey::from(n),
                // P8.1 — `#name` PrivateIdentifier as a class member
                // name. Two effects: (a) mangle the name to
                // `__priv_<ClassName>__<name>` so the existing
                // public-field machinery (struct_layouts, member
                // resolution, codegen) handles it uniformly without
                // a parallel data path; (b) force visibility to
                // Private regardless of any earlier `public`/
                // `protected` modifier — `#` is the spec marker
                // for hard-private (exact-class-only access),
                // distinct from the TS-modifier `private` which
                // also allows Protected-style subclass access.
                // Cross-class enforcement happens at typecheck
                // (check.rs P8.1-A4); ssa_lower sees a regular
                // String name (P8.1-A5 validates round-trip).
                //
                // `static #x` (P-SURF S2.37) mangles exactly like the
                // instance path — the static machinery keys off the
                // member NAME (`__sf_<C>__<mangled>` / `__sm_<C>__
                // <mangled>`), so a pre-mangled name flows through it
                // unchanged. Access sites (`C.#x` / `this.#x` in a
                // static body) mangle at `member_name_after_dot` to
                // the same `__priv_<C>__<n>`, matching the rewrite
                // table keys Pass 2.5 builds from this declaration.
                Token::PrivateIdent(n) => {
                    // ES §15.7.1 early error — `ClassElementName :
                    // PrivateIdentifier` is a Syntax Error if its
                    // StringValue is "#constructor", in EVERY member
                    // position (field, method, accessor, generator).
                    if n == "constructor" {
                        return Err(format!(
                            "class member may not be named `#constructor` in class `{name}` at {} (ES §15.7.1)",
                            self.at()
                        ));
                    }
                    let priv_name = n.clone();
                    *explicit_visibility = Some(ast::Visibility::Private);
                    // Register the RAW name on the innermost private
                    // scope so `resolve_private_refs` can bind nested
                    // `#<n>` references lexically.
                    if let Some(&sid) = self.class_stack.last() {
                        self.ast.class_private_scopes[sid as usize]
                            .1
                            .insert(priv_name.clone());
                    }
                    PropKey::from(format!("__priv_{name}__{priv_name}"))
                }
                // V3-18 wedge — accept the full reserved-word list
                // as class member names per ES spec §12.7.6
                // (PropertyName allows IdentifierName which includes
                // reserved words). Routed through the centralized
                // keyword_property_name helper so all four
                // property-name positions stay in sync.
                t if Self::keyword_property_name(t).is_some() => {
                    PropKey::from(Self::keyword_property_name(t).unwrap())
                }
                // ES §12.7.2 — the same IdentifierName written with an
                // escape (`break() {}`).
                Token::EscapedIdent(n) => PropKey::from(n),
                // RFC 20260802-class-computed-member 刀 1 — §12.7.6
                // PropertyName literal forms as direct member names:
                // `'default'() {}` / `get 'str'()` fold to the string
                // itself; `0x10() {}` / `get 1.0()` fold to the
                // §6.1.6.1.20 canonical string ("16" / "1"), the same
                // fold object literals use. Single tokens, so the
                // shared "name still unconsumed" protocol holds.
                Token::String(s) => PropKey::from(s.clone()),
                Token::Number(n) => crate::ast::number_prop_key(*n),
                t => {
                    return Err(format!(
                        "expected class member name, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        };
        Ok((member_name, consumed_computed_name))
    }
}
