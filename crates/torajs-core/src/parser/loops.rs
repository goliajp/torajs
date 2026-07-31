//! Loop / switch statement cluster (chunk 417).
//!
//! Extracted verbatim from parser.rs — the four iteration /
//! branching statement parsers:
//! - parse_while — `while (cond) body`
//! - parse_do_while — `do body while (cond);`
//! - parse_switch — `switch (scrutinee) { case ...: ... default: ... }`
//! - parse_for — `for (init?; cond?; step?) body` plus the for-of /
//!   for-in / for-await dispatch into the try_parse_for_of sibling
//!
//! The `for` doc comment had drifted onto mint_desugar_id in
//! parser.rs; re-attached to parse_for here. All four are called
//! from parse_stmt (parse_stmt.rs sibling); promoted `pub(super)`
//! per the sibling-impl pack pattern. Body unchanged.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_while(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `while`
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `while`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let cond = self.parse_expr()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after while condition, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::While { cond, body })
    }

    pub(super) fn parse_do_while(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `do`
        let body = Box::new(self.parse_stmt()?);
        match self.peek() {
            Token::While => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `while` after `do {{ … }}`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `while` in do-while, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let cond = self.parse_expr()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after do-while condition, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // Optional `;` after the closing paren.
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::DoWhile { body, cond })
    }

    pub(super) fn parse_switch(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `switch`
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `switch`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let scrutinee = self.parse_expr()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after switch scrutinee, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` to begin switch body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut cases: Vec<ast::SwitchCase> = Vec::new();
        let mut default: Option<Vec<Stmt>> = None;
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            match self.peek() {
                Token::Case => {
                    self.pos += 1;
                    let value = self.parse_expr()?;
                    match self.peek() {
                        Token::Colon => self.pos += 1,
                        t => {
                            return Err(format!(
                                "expected `:` after case value, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    let mut body: Vec<Stmt> = Vec::new();
                    while !matches!(
                        self.peek(),
                        Token::Case | Token::Default | Token::RBrace | Token::Eof
                    ) {
                        body.push(self.parse_stmt()?);
                    }
                    cases.push(ast::SwitchCase { value, body });
                }
                Token::Default => {
                    // §14.12.1 CaseBlock early error — at most one
                    // DefaultClause. The single `Option` slot would
                    // otherwise silently overwrite the first body.
                    if default.is_some() {
                        return Err(format!(
                            "multiple default clauses are not allowed at {}",
                            self.at()
                        ));
                    }
                    self.pos += 1;
                    match self.peek() {
                        Token::Colon => self.pos += 1,
                        t => {
                            return Err(format!(
                                "expected `:` after `default`, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    let mut body: Vec<Stmt> = Vec::new();
                    while !matches!(
                        self.peek(),
                        Token::Case | Token::Default | Token::RBrace | Token::Eof
                    ) {
                        body.push(self.parse_stmt()?);
                    }
                    default = Some(body);
                }
                t => {
                    return Err(format!(
                        "expected `case` or `default` inside switch, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end switch, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(Stmt::Switch {
            scrutinee,
            cases,
            default,
        })
    }

    /// `for (init?; cond?; step?) body`. Each clause is optional but the
    /// two `;` separators are required (matches TS / C). Init is parsed
    /// as a stmt (typically a `let` decl or expr-stmt). Cond is an expr.
    /// Step is an expr (we don't have post-increment yet — use
    /// `i = i + 1`).
    pub(super) fn parse_for(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `for`
        // P10.3-A1 — `for await (decl of iter)`. After consuming the
        // optional `await` keyword the rest of the head parses
        // identically to a plain `for (...)` for-of. The desugar
        // wraps the per-iteration element-load in a Member `.value`
        // access (await desugar) so the user's binding sees the
        // resolved T instead of Promise<T>. Only the for-of arm of
        // try_parse_for_of honors is_async; if the head turns out
        // to be a C-style for, we emit a clean error since `for
        // await` requires the iterable form.
        let is_async = if matches!(self.peek(), Token::Await) {
            self.pos += 1;
            true
        } else {
            false
        };
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `for`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // for-of detection: `for ( (let|const) IDENT (`:` T)? "of" EXPR )`.
        // `of` is a contextual keyword (Token::Ident("of")) so this only
        // triggers when the user means it. Falls back to C-style for-loop
        // otherwise. The desugar produces zero-overhead SSA: source is
        // bound once into a `__forof_src_N` temp (skipped if the source
        // was already an Ident — mem2reg would elide it anyway, but we
        // skip eagerly so the AST stays small), `__forof_end_N` caches
        // length, classic for-loop walks indices, body sees the user's
        // binding rebound from `__src[__i]`.
        if let Some(stmt) = self.try_parse_for_of(is_async)? {
            return Ok(stmt);
        }
        if is_async {
            return Err(format!(
                "`for await` requires the iterable form `for await (const x of iter)` at {}",
                self.at()
            ));
        }
        // init clause — empty (just `;`) or any stmt that ends with `;`.
        let init = if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
            None
        } else {
            // parse_stmt eats its own trailing `;` for let / expr stmts.
            Some(Box::new(self.parse_stmt()?))
        };
        // cond clause — empty means infinite-loop (true). Empty is `;`.
        let cond = if matches!(self.peek(), Token::Semi) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        match self.peek() {
            Token::Semi => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `;` after `for` condition, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // step clause — empty means no step.
        // V3-18 m1.h.31 — JS allows comma-separated step expressions
        // (`for (...; ...; i++, j--)`). Parse them as a chained
        // Expr::Sequence so the lowerer evaluates each in order.
        let step = if matches!(self.peek(), Token::RParen) {
            None
        } else {
            let mut s = self.parse_expr()?;
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                let next = self.parse_expr()?;
                s = self.ast.add_expr(Expr::Sequence {
                    left: s,
                    right: next,
                });
            }
            Some(s)
        };
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after `for` step, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::For {
            init,
            cond,
            step,
            body,
        })
    }
}
