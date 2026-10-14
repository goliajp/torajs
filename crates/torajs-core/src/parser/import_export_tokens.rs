//! The token-level helpers the import / export clause parsers share:
//! a ModuleExportName, the module source string, the two contextual
//! keywords (`as` / `from` / `with`), and the declaration terminator.
//!
//! Split out of [`super::import_export`] at rotation 620, when
//! widening the clause names to the full IdentifierName set pushed
//! that file past the 500-line cap. Its sibling answers "what shape
//! is this clause"; this one answers "what token stands here".

use super::*;

impl Parser<'_> {
    /// §16.2.3 ModuleExportName — a plain identifier, the reserved
    /// word `default`, or a string literal. The bool answers whether
    /// the spelling was an identifier, which is what decides whether
    /// the name can reference a LOCAL binding (a `from`-less export
    /// clause needs that; everything else does not care).
    pub(super) fn expect_module_export_name(
        &mut self,
        ctx: &str,
    ) -> Result<(String, bool), String> {
        let (name, is_ident) = match self.peek() {
            Token::Ident(n) => (n.clone(), true),
            // §16.2.3 ModuleExportName is an IdentifierName, so every
            // keyword names an export. Whether the spelling can ALSO
            // name a local binding — what a `from`-less clause needs
            // — is the narrower question: `async` and `type` are
            // ordinary names, `let` and `default` are not.
            t if Self::keyword_property_name(t).is_some() => (
                Self::keyword_property_name(t).unwrap().to_string(),
                Self::contextual_binding_name(t).is_some(),
            ),
            Token::String(s) => {
                // §16.2.3.1 — a string-spelled export name must be
                // well-formed Unicode; the WTF-8 value keeps a lone
                // surrogate as itself, so the check is direct.
                if s.has_lone_surrogates() {
                    return Err(format!(
                        "a string export name must be well-formed Unicode \
                         (lone surrogate escape) at {}",
                        self.at()
                    ));
                }
                (s.to_string_lossy_owned(), false)
            }
            t => {
                return Err(format!(
                    "expected an export name after {ctx}, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        Ok((name, is_ident))
    }

    /// The string literal after a `from` keyword in a re-export clause.
    pub(super) fn expect_module_source(&mut self) -> Result<String, String> {
        match self.peek() {
            Token::String(s) => {
                let s = s.to_string_lossy_owned();
                self.pos += 1;
                Ok(s)
            }
            t => Err(format!(
                "expected module source string after `from`, got {t:?} at {}",
                self.at()
            )),
        }
    }

    pub(super) fn expect_ident_keyword(&mut self, kw: &str) -> Result<(), String> {
        match self.peek() {
            Token::Ident(_) if self.at_word(kw) => {
                self.pos += 1;
                Ok(())
            }
            t => Err(format!("expected `{kw}`, got {t:?} at {}", self.at())),
        }
    }

    /// §16.2.2 NamedImports / §16.2.3 NamedExports — specifiers are
    /// comma-separated, so `{a \u0061s b}` is not three of them: after
    /// one, only a `,` or the closing `}` may follow.
    pub(super) fn end_specifier(&mut self) -> Result<(), String> {
        match self.peek() {
            Token::Comma => self.pos += 1,
            Token::RBrace => {}
            t => {
                return Err(format!(
                    "expected `,` or `}}` after a specifier, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(())
    }

    /// §16.2.3 — what `export default` heads at `head` decides how it
    /// ends: a HoistableDeclaration or ClassDeclaration closes at its
    /// own `}`, while an AssignmentExpression needs the `;` that ASI
    /// may supply (§12.10.1). So `export default \u0061sync function
    /// f() {}` — an escaped `async` is the identifier, not the keyword —
    /// is an expression followed by a stray declaration.
    pub(super) fn end_export_default(&mut self, head: usize) -> Result<(), String> {
        let decl = match &self.tokens[head].token {
            Token::Function | Token::Class => true,
            Token::Async => matches!(
                self.tokens.get(head + 1).map(|t| &t.token),
                Some(Token::Function)
            ),
            _ => false,
        };
        if !decl {
            return self.expect_asi_end("an `export default` expression");
        }
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(())
    }

    /// §16.2 — an ImportDeclaration / ExportDeclaration ends at its
    /// module source. Whatever comes next has to be able to START a
    /// statement, which means a `;`, a `}`, end of input, or a
    /// LineTerminator for ASI to insert the semicolon itself. So
    /// `export * from "m" null;` is a SyntaxError rather than two
    /// statements — the tolerant reading silently accepted source no
    /// engine does.
    pub(super) fn expect_decl_end(&mut self, what: &str) -> Result<(), String> {
        self.parse_with_clause()?;
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
            return Ok(());
        }
        if matches!(self.peek(), Token::Eof | Token::RBrace) || self.has_newline_before(self.pos) {
            return Ok(());
        }
        Err(format!(
            "expected `;` or a line break after {what}, got {:?} at {}",
            self.peek(),
            self.at()
        ))
    }
}
