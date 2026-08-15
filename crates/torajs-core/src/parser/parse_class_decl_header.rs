//! Class-decl header parsing (name / generic type params / heritage
//! clauses + class-body-opening `{`), split from
//! `parse_class_decl.rs::parse_class_decl_with_abstract` (2026-07-03,
//! fn-debt decomp). Bodies verbatim; mechanical rewrite: each
//! segment tail gains its `Ok(..)` return.

use super::*;

impl<'a> Parser<'a> {
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
            // downstream resolution. Inner self-binding (Inner referring
            // to the class inside its own body) is an L3b follow-up.
            Token::Ident(n) if force_synth => {
                // RFC 20260714-dstr-residual blade 4 — the discarded
                // inner name is still the class's `.name` (§15.5.5:
                // a named class expression keeps its self-name over
                // any binding). Record it in the display channel; it
                // lands first, so binding-position or_insert
                // registrations never override it. Inner self-binding
                // resolution stays an L3b follow-up.
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
            // `extends Base<number>` — heritage type arguments,
            // consumed and discarded like `implements` (TS §3.7: no
            // runtime effect). At heritage position a `<` cannot open
            // a comparison — the clause is a LeftHandSideExpression —
            // so there is no `f < 3` ambiguity to rewind for.
            if matches!(self.peek(), Token::Lt) {
                self.pos += 1;
                let _ = self.parse_type_args_list()?;
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
