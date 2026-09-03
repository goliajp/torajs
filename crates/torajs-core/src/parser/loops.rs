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
    /// §14.6 / §14.7 / §14.13 early error — a single-statement
    /// position (if/else branch, loop body, labeled body) admits a
    /// Statement, not a Declaration. Lexical declarations (let /
    /// const / class / generator / async fn) reject; a plain
    /// `function` is allowed (Annex B.3.2/B.3.4 — bun accepts both
    /// `if (x) function f(){}` and `lbl: function f(){}`), and `var`
    /// is a VariableStatement proper. The check is post-parse on the
    /// produced Stmt — parse side effects are moot since the whole
    /// compile aborts.
    ///
    /// `admits_labelled_fn` separates the two readings of a label
    /// chain. §14.13 LabelledItem may itself be a FunctionDeclaration
    /// (annexB B.3.2), so `l1: l2: function f(){}` is legal as a
    /// statement-list item — that is the labelled-statement body's own
    /// position, and it passes `true`. Every other position here takes
    /// a Statement, where §14.13.1 makes it a Syntax Error if
    /// IsLabelledFunction(Statement) — annexB extends the bare
    /// `if (x) function f(){}` spelling, never the labelled one. The
    /// chain is transparent, so those positions walk it.
    pub(super) fn reject_decl_in_single_stmt(
        &self,
        body: &Stmt,
        body_start: usize,
        ctx: &str,
        admits_labelled_fn: bool,
    ) -> Result<(), String> {
        if !admits_labelled_fn && let Some(kind) = Self::labelled_fn_kind(body) {
            return Err(format!(
                "a labeled {kind} is not allowed as the body of {ctx} at {} \
                 (ES §14.13.1)",
                self.at()
            ));
        }
        let offending = match body {
            Stmt::LetDecl {
                is_var: false,
                mutable,
                ..
            } => {
                // Sloppy-mode `let \n x = 1` is an ASI-split
                // ExpressionStatement pair (`let` the identifier),
                // not a LexicalDeclaration — the §13.16 lookahead
                // only forbids `let [`. tr misparses the shape into
                // one LetDecl; rejecting it here turned a
                // runs-fine program into a parse error (test262
                // let-identifier-with-newline family), so the
                // newline spelling keeps the historical behavior.
                if *mutable && self.let_newline_asi_form(body_start) {
                    None
                } else {
                    Some(if *mutable { "let" } else { "const" })
                }
            }
            // RFC 20260809 knife 3 residue — §14.7/§14.13 take a
            // Statement; a UsingDeclaration (single or the Multi a
            // multi-binding head parses to) is not one
            // (with-initializer-for/do/while-statement family).
            Stmt::UsingDecl { .. } => Some("using"),
            Stmt::Multi(inner) if inner.iter().any(|s| matches!(s, Stmt::UsingDecl { .. })) => {
                Some("using")
            }
            Stmt::ClassDecl { .. } => Some("class"),
            Stmt::FnDecl {
                name, is_generator, ..
            } => {
                if *is_generator {
                    Some("generator function")
                } else if self.ast.async_fns.contains(name) {
                    Some("async function")
                } else {
                    None
                }
            }
            _ => None,
        };
        match offending {
            Some(kind) => Err(format!(
                "{kind} declarations are not allowed as the body of {ctx} at {}",
                self.at()
            )),
            None => Ok(()),
        }
    }

    /// §14.13.1 IsLabelledFunction — walk a label chain and answer
    /// what it wraps, when that is a function declaration. The chain
    /// may be any depth (`l1: l2: l3: function f(){}`), which is why
    /// this recurses rather than peeking one level.
    fn labelled_fn_kind(body: &Stmt) -> Option<&'static str> {
        let Stmt::Labeled { body, .. } = body else {
            return None;
        };
        match &**body {
            Stmt::FnDecl { .. } => Some("function declaration"),
            inner => Self::labelled_fn_kind(inner),
        }
    }

    /// `let` at `body_start` followed by a LINE-BREAK then an
    /// identifier — the ASI-identifier spelling the reject above
    /// exempts. (`var` shares `Token::Let` at the token level but
    /// its LetDecl carries `is_var: true` and never reaches this.)
    pub(super) fn let_newline_asi_form(&self, body_start: usize) -> bool {
        let (Some(a), Some(b)) = (self.tokens.get(body_start), self.tokens.get(body_start + 1))
        else {
            return false;
        };
        matches!(a.token, Token::Let)
            && matches!(b.token, Token::Ident(_))
            && self.source[a.span.end as usize..b.span.start as usize].contains('\n')
    }
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
        // Re-evaluated every iteration — hoisting a yield out of it
        // would run the yield exactly once.
        let cond = self.with_yield_hoist_disallowed(|p| p.parse_expr())?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after while condition, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let body_start = self.pos;
        let body = Box::new(self.parse_stmt()?);
        self.reject_decl_in_single_stmt(&body, body_start, "a while loop", false)?;
        Ok(Stmt::While { cond, body })
    }

    pub(super) fn parse_do_while(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `do`
        let body_start = self.pos;
        let body = Box::new(self.parse_stmt()?);
        self.reject_decl_in_single_stmt(&body, body_start, "a do-while loop", false)?;
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
        // Per-iteration position, same as the while condition.
        let cond = self.with_yield_hoist_disallowed(|p| p.parse_expr())?;
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
                    // Case values evaluate in order until one matches
                    // — conditional position, no yield hoist.
                    let value = self.with_yield_hoist_disallowed(|p| p.parse_expr())?;
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
                        // §14.12 (ES2026 ERM) — a UsingDeclaration
                        // may not sit DIRECTLY in a case clause's
                        // statement list (wrap it in a block). The
                        // head test keeps `using[i]` / ASI forms as
                        // expressions, and a Block re-enters
                        // parse_stmt fresh, so `case 0: { using x =
                        // … }` stays legal.
                        if self.using_decl_head().is_some() {
                            return Err(format!(
                                "`using` declarations are not allowed directly in a case clause at {}",
                                self.at()
                            ));
                        }
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
                        // Same §14.12 clause gate as `case` above.
                        if self.using_decl_head().is_some() {
                            return Err(format!(
                                "`using` declarations are not allowed directly in a default clause at {}",
                                self.at()
                            ));
                        }
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
            // §14.7.4 — the init position parses without [In]
            // (`#x in o` refuses here); error paths don't restore,
            // following `current_class`.
            self.in_for_init = true;
            // parse_stmt eats its own trailing `;` for let / expr stmts.
            let s = self.parse_stmt()?;
            self.in_for_init = false;
            Some(Box::new(s))
        };
        // cond clause — empty means infinite-loop (true). Empty is `;`.
        // Per-iteration position: no yield hoist (see parse_while).
        let cond = if matches!(self.peek(), Token::Semi) {
            None
        } else {
            Some(self.with_yield_hoist_disallowed(|p| p.parse_expr())?)
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
        // Per-iteration position: no yield hoist.
        let step = if matches!(self.peek(), Token::RParen) {
            None
        } else {
            let mut s = self.with_yield_hoist_disallowed(|p| p.parse_expr())?;
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                let next = self.with_yield_hoist_disallowed(|p| p.parse_expr())?;
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
        let body_start = self.pos;
        let body = Box::new(self.parse_stmt()?);
        self.reject_decl_in_single_stmt(&body, body_start, "a for loop", false)?;
        Ok(Stmt::For {
            init,
            cond,
            step,
            body,
        })
    }
}
