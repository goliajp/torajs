//! Import / export declaration cluster (chunk 418).
//!
//! Extracted verbatim from parser.rs — the Phase K.1 module-syntax
//! parsers plus their two private helpers:
//! - parse_import — `import "./x"` / default / `* as ns` / named /
//!   combined clauses
//! - parse_export — `export <decl>` / `export { a, b as c }` /
//!   `export default <expr>`
//! - expect_ident_keyword — contextual-keyword matcher (`from` / `as`)
//! - skip_optional_semi — trailing `;` tolerance
//! - expect_decl_end — §16.2 declaration terminator (`;` / `}` / EOF /
//!   a line break for ASI)
//!
//! parse_import + parse_export are called from parse_stmt
//! (parse_stmt.rs sibling); the helpers are internal to this
//! cluster. All promoted `pub(super)` per the sibling-impl pack
//! pattern. Body unchanged.

use super::*;
use crate::ast::ExportStar;

impl<'a> Parser<'a> {
    /// Phase K.1 — `import` declaration parser. Single-file mode: builds
    /// the AST node so the syntax is accepted; the lowerer treats it as
    /// a no-op until K.2 wires in cross-file linking. Recognized shapes:
    ///   - `import "./x"`                       (side-effect-only)
    ///   - `import x from "./x"`                (default import)
    ///   - `import * as ns from "./x"`          (namespace import)
    ///   - `import { a, b as c } from "./x"`    (named imports)
    ///   - `import x, { a, b } from "./x"`      (combined default + named)
    ///   - `import x, * as ns from "./x"`       (combined default + namespace)
    pub(super) fn parse_import(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `import`
        let mut default: Option<String> = None;
        let mut namespace: Option<String> = None;
        let mut named: Vec<(String, Option<String>)> = Vec::new();
        // Bare `import "./x"` — no clause, just the source.
        if let Token::String(_) = self.peek() {
            let source = match self.peek() {
                Token::String(s) => s.to_string_lossy_owned(),
                _ => unreachable!(),
            };
            self.pos += 1;
            self.expect_decl_end("an import declaration")?;
            return Ok(Stmt::ImportDecl {
                default,
                namespace,
                named,
                source,
            });
        }
        // import-defer proposal — `import defer * as ns from "..."`.
        // `defer` is a CONTEXTUAL keyword: it only reads as one when
        // the very next token is `*` at the head of the clause;
        // `import defer from "./x"` keeps `defer` as a plain default
        // binding name (test262 valid-default-binding-named-defer).
        // Deferred evaluation is layered — this parse accepts the
        // form and the module resolves EAGERLY through the normal
        // namespace lane; the laziness itself waits for a lazy
        // module-init substrate (roadmap phase, not dropped scope).
        if matches!(self.peek(), Token::Ident(n) if n == "defer")
            && matches!(self.tokens[self.pos + 1].token, Token::Star)
        {
            self.pos += 1; // consume `defer`
        }
        // Default import: `import x ...` (next token is Ident).
        let mut default_comma = false;
        if let Token::Ident(_) = self.peek() {
            let name = match self.peek() {
                Token::Ident(n) => n.clone(),
                _ => unreachable!(),
            };
            self.pos += 1;
            default = Some(name);
            // Optional `, { ... }` or `, * as ns`.
            if matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                default_comma = true;
            }
        }
        // §16.2.2 ImportClause — a namespace / named clause after a
        // default binding requires the `,` separator; without it
        // `import defer { x } from` (test262 invalid-defer-named)
        // and `import x { y } from` parsed as if the comma were
        // there.
        if default.is_some() && !default_comma && matches!(self.peek(), Token::Star | Token::LBrace)
        {
            return Err(format!(
                "expected `,` or `from` after default import binding, got {:?} at {}",
                self.peek(),
                self.at()
            ));
        }
        // Namespace: `* as ns` (Token::Star + Ident("as") + Ident).
        if matches!(self.peek(), Token::Star) {
            self.pos += 1;
            self.expect_ident_keyword("as")?;
            let n = match self.peek() {
                Token::Ident(n) => n.clone(),
                t => {
                    return Err(format!(
                        "expected namespace ident after `* as`, got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            self.pos += 1;
            namespace = Some(n);
        }
        // Named: `{ a, b as c }`.
        if matches!(self.peek(), Token::LBrace) {
            self.pos += 1;
            while !matches!(self.peek(), Token::RBrace) {
                let orig = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    // §16.2.2 ImportSpecifier — ModuleExportName
                    // covers reserved words; `default as x` re-binds
                    // the default export under a local name.
                    Token::Default => "default".to_string(),
                    t => {
                        return Err(format!(
                            "expected ident in import named clause, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                let alias = if matches!(self.peek(), Token::Ident(n) if n == "as") {
                    self.pos += 1;
                    let a = match self.peek() {
                        Token::Ident(n) => n.clone(),
                        t => {
                            return Err(format!(
                                "expected alias ident after `as`, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    };
                    self.pos += 1;
                    Some(a)
                } else {
                    None
                };
                named.push((orig, alias));
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                }
            }
            self.pos += 1; // consume `}`
        }
        // `default as x` rides the resolver's default lane — it IS
        // the default binding (§16.2.2). Bare `{ default }` is a
        // syntax error: `default` is a reserved word, not a legal
        // ImportedBinding. When a default binding is ALSO present
        // (`import d, { default as x }`) the named entry stays; its
        // lookup misses every named export and drops silently
        // (recorded subset boundary — the double-binding form).
        if let Some(idx) = named.iter().position(|(o, _)| o == "default") {
            match (&named[idx].1, &default) {
                (None, _) => {
                    return Err(format!(
                        "`default` in an import named clause requires `as <binding>` at {}",
                        self.at()
                    ));
                }
                (Some(alias), None) => {
                    default = Some(alias.clone());
                    named.remove(idx);
                }
                (Some(_), Some(_)) => {}
            }
        }
        // `from "./x"` tail.
        self.expect_ident_keyword("from")?;
        let source = match self.peek() {
            Token::String(s) => s.to_string_lossy_owned(),
            t => {
                return Err(format!(
                    "expected string source after `from`, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        self.expect_decl_end("an import declaration")?;
        Ok(Stmt::ImportDecl {
            default,
            namespace,
            named,
            source,
        })
    }

    /// Phase K.1 — `export` declaration parser. Recognized shapes:
    ///   - `export function/class/type/const/let X ...`  (modifier on decl)
    ///   - `export { a, b as c }`                        (named re-export)
    ///   - `export * from "./b"`                          (star re-export)
    ///   - `export * as ns from "./b"`                    (namespace re-export)
    ///   - `export default <expr>`                        (default export)
    pub(super) fn parse_export(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `export`
        // `export default <expr>;`
        if matches!(self.peek(), Token::Default) {
            self.pos += 1;
            let e = self.parse_expr()?;
            self.skip_optional_semi();
            return Ok(Stmt::ExportDecl {
                inner: None,
                named: Vec::new(),
                default_expr: Some(e),
                source: None,
                star: None,
            });
        }
        // §16.2.3 ExportFromClause — `export * from "m"` and
        // `export * as ns from "m"`. Both bind nothing locally, so
        // `inner` / `named` stay empty and the star head carries the
        // shape; the `from` clause is mandatory for either form.
        if matches!(self.peek(), Token::Star) {
            return self.parse_export_star();
        }
        // `export { a, b as c };` — both halves of a specifier are
        // §16.2.3 ModuleExportName, which covers reserved words
        // (`default`) and string literals alongside plain identifiers.
        if matches!(self.peek(), Token::LBrace) {
            self.pos += 1;
            let mut named: Vec<(String, Option<String>)> = Vec::new();
            // The first orig spelled by something that cannot be an
            // IdentifierReference — legal only under a `from` clause,
            // where the name looks up in the other module instead of
            // the local scope (§16.2.3.1 early error otherwise).
            let mut non_ident_ref: Option<String> = None;
            while !matches!(self.peek(), Token::RBrace) {
                let (orig, orig_is_ident) =
                    self.expect_module_export_name("export named clause")?;
                if !orig_is_ident && non_ident_ref.is_none() {
                    non_ident_ref = Some(orig.clone());
                }
                let alias = if matches!(self.peek(), Token::Ident(n) if n == "as") {
                    self.pos += 1;
                    Some(self.expect_module_export_name("`as`")?.0)
                } else {
                    None
                };
                named.push((orig, alias));
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                }
            }
            self.pos += 1; // consume `}`
            // P13-S4 — optional `from "./b"` for re-export form.
            let source = if matches!(self.peek(), Token::Ident(n) if n == "from") {
                self.pos += 1;
                Some(self.expect_module_source()?)
            } else {
                None
            };
            if source.is_none()
                && let Some(n) = non_ident_ref
            {
                return Err(format!(
                    "`{n}` in an export clause without `from` references a local binding, \
                     which a reserved word or string cannot name, at {}",
                    self.at()
                ));
            }
            self.expect_decl_end("an export declaration")?;
            return Ok(Stmt::ExportDecl {
                inner: None,
                named,
                default_expr: None,
                source,
                star: None,
            });
        }
        // `export <decl>` — modifier on a function / class / type / let
        // / const declaration. Single-file mode just unwraps the inner
        // decl; the AST-level wrapper is preserved for future K.2 work.
        let inner = self.parse_stmt()?;
        Ok(Stmt::ExportDecl {
            inner: Some(Box::new(inner)),
            named: Vec::new(),
            default_expr: None,
            source: None,
            star: None,
        })
    }

    /// §16.2.3 `export * from "m"` / `export * as ns from "m"`, entered
    /// with the cursor on the `*`. The name after `as` is a
    /// ModuleExportName, so a string literal (`export * as "a-b" from`)
    /// and the reserved word `default` (`export * as default from`) are
    /// both legal spellings alongside a plain identifier.
    fn parse_export_star(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `*`
        let star = if matches!(self.peek(), Token::Ident(n) if n == "as") {
            self.pos += 1;
            ExportStar::AsNamespace(self.expect_module_export_name("`* as`")?.0)
        } else {
            ExportStar::All
        };
        self.expect_ident_keyword("from")?;
        let source = self.expect_module_source()?;
        self.expect_decl_end("an export declaration")?;
        Ok(Stmt::ExportDecl {
            inner: None,
            named: Vec::new(),
            default_expr: None,
            source: Some(source),
            star: Some(star),
        })
    }

    /// §16.2.2 WithClause — `with { type: "json" }` after a module
    /// source. The attributes select how the host reads the module, so
    /// nothing here can act on them until there is more than one module
    /// type to select; what matters now is that the clause belongs to
    /// the declaration. Read as a statement it became a `with` block,
    /// and once `expect_decl_end` started guarding the tail it became a
    /// SyntaxError — both wrong for source every engine accepts.
    ///
    /// A repeated AttributeKey is an early error (§16.2.2 static
    /// semantics), and the key may be spelled as a string literal.
    fn parse_with_clause(&mut self) -> Result<(), String> {
        if !matches!(self.peek(), Token::Ident(s) if s == "with") {
            return Ok(());
        }
        if !matches!(self.tokens[self.pos + 1].token, Token::LBrace) {
            return Ok(());
        }
        self.pos += 2; // consume `with` `{`
        let mut keys: Vec<String> = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            let key = match self.peek() {
                Token::Ident(n) => n.clone(),
                Token::String(s) => s.to_string_lossy_owned(),
                t => {
                    return Err(format!(
                        "expected an import-attribute key, got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            self.pos += 1;
            if keys.contains(&key) {
                return Err(format!(
                    "duplicate import attribute `{key}` at {}",
                    self.at()
                ));
            }
            keys.push(key);
            if !matches!(self.peek(), Token::Colon) {
                return Err(format!(
                    "expected `:` after an import-attribute key, got {:?} at {}",
                    self.peek(),
                    self.at()
                ));
            }
            self.pos += 1;
            if !matches!(self.peek(), Token::String(_)) {
                return Err(format!(
                    "an import-attribute value must be a string literal, got {:?} at {}",
                    self.peek(),
                    self.at()
                ));
            }
            self.pos += 1;
            if matches!(self.peek(), Token::Comma) {
                self.pos += 1;
            } else {
                break;
            }
        }
        if !matches!(self.peek(), Token::RBrace) {
            return Err(format!(
                "expected `}}` to close an import-attribute clause, got {:?} at {}",
                self.peek(),
                self.at()
            ));
        }
        self.pos += 1;
        Ok(())
    }

    /// §16.2.3 ModuleExportName — a plain identifier, the reserved
    /// word `default`, or a string literal. The bool answers whether
    /// the spelling was an identifier, which is what decides whether
    /// the name can reference a LOCAL binding (a `from`-less export
    /// clause needs that; everything else does not care).
    fn expect_module_export_name(&mut self, ctx: &str) -> Result<(String, bool), String> {
        let (name, is_ident) = match self.peek() {
            Token::Ident(n) => (n.clone(), true),
            Token::Default => ("default".to_string(), false),
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
    fn expect_module_source(&mut self) -> Result<String, String> {
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
            Token::Ident(n) if n == kw => {
                self.pos += 1;
                Ok(())
            }
            t => Err(format!("expected `{kw}`, got {t:?} at {}", self.at())),
        }
    }

    pub(super) fn skip_optional_semi(&mut self) {
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
    }

    /// §16.2 — an ImportDeclaration / ExportDeclaration ends at its
    /// module source. Whatever comes next has to be able to START a
    /// statement, which means a `;`, a `}`, end of input, or a
    /// LineTerminator for ASI to insert the semicolon itself. So
    /// `export * from "m" null;` is a SyntaxError rather than two
    /// statements — the tolerant reading silently accepted source no
    /// engine does.
    fn expect_decl_end(&mut self, what: &str) -> Result<(), String> {
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
