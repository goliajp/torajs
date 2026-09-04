//! Class-decl header parsing (name / generic type params / heritage
//! clauses + class-body-opening `{`), split from
//! `parse_class_decl.rs::parse_class_decl_with_abstract` (2026-07-03,
//! fn-debt decomp). Bodies verbatim; mechanical rewrite: each
//! segment tail gains its `Ok(..)` return.

use super::*;

/// Everything `parse_class_decl_with_abstract` needs out of the
/// header sequence — bindings plus the saved parser state its exit
/// paths restore.
pub(super) struct ClassHeader {
    pub name: String,
    pub type_params: Vec<String>,
    pub parent: Option<ExprId>,
    pub parent_name: Option<String>,
    pub saved_class: Option<String>,
    pub saved_super_prop: bool,
    pub saved_has_parent: bool,
}

impl<'a> Parser<'a> {
    /// The whole class-decl header sequence: `class` + name + type
    /// params + heritage + the private-scope push + the saved parser
    /// state. Order is load-bearing twice over: `current_class` /
    /// `super_prop_allowed` set before anything of the body parses,
    /// and the heritage parses BEFORE this class's private scope opens
    /// — §15.7.14 evaluates ClassHeritage in the class-OUTER private
    /// environment, so `class C extends (obj.#x) {}` resolves `#x`
    /// against the enclosing class (or fails the early error), never
    /// against C's own names (the test262 early-error family
    /// grammar-private-environment-on-class-heritage-* pins the
    /// phase; rotation 409, surfaced when the heritage widened from a
    /// bare name to an expression).
    pub(super) fn parse_class_header(
        &mut self,
        allow_anon: bool,
        force_synth: bool,
    ) -> Result<ClassHeader, String> {
        self.pos += 1; // consume `class`
        let name = self.parse_class_name(allow_anon, force_synth)?;
        // P8.1 — set `current_class` so private-member parsing inside
        // this class body can mangle `this.#x` to `__priv_<class>__x`.
        // Saved/restored at every successful return path; on `return
        // Err(...)` we don't restore (parse has failed; parser state
        // is moot). Cloned for nested classes — the recursive call
        // overwrites; the caller restores the outer name on exit.
        let saved_class = self.current_class.take();
        self.current_class = Some(name.clone());
        // r334 blade 6 — SuperProperty (`super.x` et al) is legal
        // throughout a class body (each part has a [[HomeObject]], ES
        // §15.7.14). Ordinary function bodies nested inside re-clear
        // it at their own parse sites; arrows inherit.
        let saved_super_prop = std::mem::replace(&mut self.super_prop_allowed, true);
        let type_params = self.parse_class_type_params()?;
        let parent = self.parse_class_heritage()?;
        // Private-name lexical scope for the body (ES §15.7 — nested
        // classes see outer `#x` names, an inner redeclaration
        // shadows). Declarations fill the set as members parse; `#x`
        // REFERENCES defer to `resolve_private_refs`.
        let scope_id = self.ast.class_private_scopes.len() as u32;
        self.ast
            .class_private_scopes
            .push((name.clone(), std::collections::HashSet::new()));
        self.class_stack.push(scope_id);
        // The generator-method branch keys its `super` rewrites on the
        // statically-known parent NAME; a non-Ident heritage answers
        // None there, same as no heritage (those classes are routed to
        // the value-shaped-parent lane before any of this matters).
        let parent_name: Option<String> = self.ast.parent_ident_name(parent).map(str::to_string);
        // Read by the constructor branch, to decide whether a
        // `super()` in that body is the legal one (ES §15.7.1).
        let saved_has_parent = self.current_class_has_parent;
        self.current_class_has_parent = parent.is_some();
        Ok(ClassHeader {
            name,
            type_params,
            parent,
            parent_name,
            saved_class,
            saved_super_prop,
            saved_has_parent,
        })
    }

    /// Class name position: user ident, force-synth discard (P8.5
    /// class-expression inner name), or anonymous synth mint.
    ///
    /// r380 — the §12.7.2 judge runs with `strict` hardwired true:
    /// §15.7 makes every part of a class strict whatever the goal
    /// said and whatever the enclosing function said, so `class
    /// package {}` is a SyntaxError in a sloppy script too (measured:
    /// bun refuses all seven words here in both goals, tr took them
    /// all). The class-body `class_stack` push happens after this
    /// call, which is why the verdict is passed in rather than read
    /// off the stack.
    pub(super) fn parse_class_name(
        &mut self,
        allow_anon: bool,
        force_synth: bool,
    ) -> Result<String, String> {
        let name = match self.peek() {
            // P8.5 — `force_synth`: even when a class-expression-position
            // class carries an inner name (`class Inner { ... }`),
            // consume-and-discard it so the synth name controls all
            // downstream resolution. The inner self-binding — `Inner`
            // referring to the class from inside its own body — is
            // restored downstream by `class_globals_shadow`, which
            // reads the display channel below and rewrites the body's
            // own references to the synth sentinel, under the same
            // shadowing rules the declaration half gets.
            Token::Ident(n) if force_synth => {
                // RFC 20260714-dstr-residual blade 4 — the discarded
                // inner name is still the class's `.name` (§15.5.5:
                // a named class expression keeps its self-name over
                // any binding). Record it in the display channel; it
                // lands first, so binding-position or_insert
                // registrations never override it, and it is also the
                // channel the body's self-references resolve through.
                // `implements` is TypeScript's heritage keyword, and
                // at EXPRESSION position it reads as one: bun takes
                // `const C = class implements {}` as an anonymous
                // class carrying a heritage clause, while refusing it
                // at declaration position where a name is required.
                // The other six words have no such second reading, so
                // only this one is left to whatever the parser
                // already did with it (which is not bun's reading
                // either — recorded as 380-04, not widened here).
                if n != "implements" {
                    self.reject_if_strict_reserved(n, true)?;
                }
                let inner = n.clone();
                self.pos += 1;
                let id = self.mint_desugar_id();
                let synth = format!("__ClassExpr_{id}");
                self.ast
                    .class_expr_display_names
                    .insert(synth.clone(), inner);
                synth
            }
            Token::Ident(n) => {
                self.reject_if_strict_reserved(n, true)?;
                let n = n.clone();
                self.pos += 1;
                n
            }
            // P8.5 — anonymous class expression (`const F = class { ... }`,
            // `new (class { ... })()`). Mint a unique synth name that
            // `synthesize_class_globals` will expose as
            // `__class___ClassExpr_<id>`. Source-side anonymous → user
            // code never references this name directly; only the
            // parser-emitted Ident at the original use site does.
            _ if allow_anon => {
                let id = self.mint_desugar_id();
                format!("__ClassExpr_{id}")
            }
            t => {
                return Err(format!("expected class name, got {t:?} at {}", self.at()));
            }
        };
        Ok(name)
    }

    /// Optional `class C<T, ...>` generic type-param list.
    pub(super) fn parse_class_type_params(&mut self) -> Result<Vec<String>, String> {
        let mut type_params: Vec<String> = Vec::new();
        if matches!(self.peek(), Token::Lt) {
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
                                "expected type-param name in class<...>, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    match self.peek() {
                        Token::Comma => self.pos += 1,
                        Token::Gt => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `>` in class type params, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
            }
            match self.peek() {
                Token::Gt => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `>` to close class type params, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        Ok(type_params)
    }

    /// Optional `extends P` clause + `implements I, ...` clause
    /// (V3-18: consumed and discarded per TS spec §3.7); also
    /// consumes the class-body-opening `{`.
    pub(super) fn parse_class_heritage(&mut self) -> Result<Option<ExprId>, String> {
        // M5.2 / §15.7 — optional `extends LeftHandSideExpression`
        // clause (RFC 20260815-heritage-exprid), parsed at postfix
        // level like every other LHS position.
        let parent: Option<ExprId> = if matches!(self.peek(), Token::Extends) {
            self.pos += 1;
            // §15.7 ClassHeritage is a LeftHandSideExpression — a BARE
            // arrow (async included) is an AssignmentExpression and a
            // SyntaxError here; a parenthesized one is a
            // PrimaryExpression and fine. `is_arrow_fn_at_lparen` scans
            // to the MATCHING `)`, so `((o) => {})` answers false (the
            // outer close is followed by the class-body `{`). The
            // test262 class-heritage-*-arrow-heritage early errors pin
            // the phase: parse, not evaluation.
            let bare_arrow = match self.peek() {
                Token::LParen => self.is_arrow_fn_at_lparen(),
                Token::Ident(_) => matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.token),
                    Some(Token::FatArrow)
                ),
                // `async` is its own token, never an Ident.
                Token::Async => {
                    let saved = self.pos;
                    self.pos += 1;
                    let is = (matches!(self.peek(), Token::LParen) && self.is_arrow_fn_at_lparen())
                        || (matches!(self.peek(), Token::Ident(_))
                            && matches!(
                                self.tokens.get(self.pos + 1).map(|t| &t.token),
                                Some(Token::FatArrow)
                            ));
                    self.pos = saved;
                    is
                }
                _ => false,
            };
            if bare_arrow {
                return Err(format!(
                    "class heritage is a LeftHandSideExpression; a bare arrow \
                     function is not allowed here (at {})",
                    self.at()
                ));
            }
            let rhs = self.parse_postfix()?;
            // P8.5 — a bare-name heritage over a class-expression
            // binding (`var A = class {}`) is a class VALUE under a
            // user name; the parser resolves it to the synth class the
            // same way `new A()` and `A.m()` already do. Without this
            // the field-flattening pass looks up `A` among declared
            // class NAMES, finds nothing, and rejects the whole
            // program as a forward reference — even though `A` was
            // bound above. Applying the alias HERE keeps every static
            // consumer reading back exactly the name it did when the
            // heritage was a plain `Option<String>`.
            let rhs = match self.ast.get_expr(rhs) {
                Expr::Ident(n) => match self.class_value_aliases.get(n) {
                    Some(alias) => {
                        let alias = alias.clone();
                        self.ast.add_expr(Expr::Ident(alias))
                    }
                    None => rhs,
                },
                _ => rhs,
            };
            // `extends Base<number>` — heritage type arguments. At
            // heritage position a `<` cannot open a comparison — the
            // clause is a LeftHandSideExpression — so there is no
            // `f < 3` ambiguity to rewind for. Recorded (keyed by the
            // extending class, set by `parse_class_header` before the
            // heritage parses) for the field-flattening substitution:
            // a generic parent's inherited field types spell ITS type
            // params, which resolve nowhere in the subclass.
            if matches!(self.peek(), Token::Lt) {
                self.pos += 1;
                let args = self.parse_type_args_list()?;
                if let Some(cls) = self.current_class.clone() {
                    self.ast.class_parent_type_args.insert(cls, args);
                }
            }
            Some(rhs)
        } else {
            None
        };
        // V3-18 wedge — `implements Foo, Bar` clause on class.
        // Per TS spec §3.7, `implements` declares structural-typing
        // intent without runtime effect. Subset consumes and
        // discards the list — the structural check is provided by
        // existing field-by-field typecheck on assignment.
        if let Token::Ident(s) = self.peek()
            && s == "implements"
        {
            self.pos += 1;
            loop {
                let _iface = self.parse_type_ann()?;
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                    continue;
                }
                break;
            }
        }
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` to begin class body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(parent)
    }
}
