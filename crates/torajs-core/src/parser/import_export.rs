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
                Token::String(s) => s.clone(),
                _ => unreachable!(),
            };
            self.pos += 1;
            self.skip_optional_semi();
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
            Token::String(s) => s.clone(),
            t => {
                return Err(format!(
                    "expected string source after `from`, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        self.skip_optional_semi();
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
        // `export { a, b as c };`
        if matches!(self.peek(), Token::LBrace) {
            self.pos += 1;
            let mut named: Vec<(String, Option<String>)> = Vec::new();
            while !matches!(self.peek(), Token::RBrace) {
                let orig = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    t => {
                        return Err(format!(
                            "expected ident in export named clause, got {t:?} at {}",
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
            // P13-S4 — optional `from "./b"` for re-export form.
            let source = if matches!(self.peek(), Token::Ident(n) if n == "from") {
                self.pos += 1;
                Some(self.expect_module_source()?)
            } else {
                None
            };
            self.skip_optional_semi();
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
            let name = match self.peek() {
                Token::Ident(n) => n.clone(),
                Token::String(s) => s.clone(),
                Token::Default => "default".to_string(),
                t => {
                    return Err(format!(
                        "expected export name after `* as`, got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            self.pos += 1;
            ExportStar::AsNamespace(name)
        } else {
            ExportStar::All
        };
        self.expect_ident_keyword("from")?;
        let source = self.expect_module_source()?;
        self.skip_optional_semi();
        Ok(Stmt::ExportDecl {
            inner: None,
            named: Vec::new(),
            default_expr: None,
            source: Some(source),
            star: Some(star),
        })
    }

    /// The string literal after a `from` keyword in a re-export clause.
    fn expect_module_source(&mut self) -> Result<String, String> {
        match self.peek() {
            Token::String(s) => {
                let s = s.clone();
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
}
