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
//!
//! The single-statement-position judge the three loop bodies ask
//! moved to the `single_stmt_judge` sibling in rotation 578.

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
        // Re-evaluated every iteration — hoisting a yield out of it
        // would run the yield exactly once.
        // A yield in the condition runs per iteration, so its hoisted
        // statements go to the top of the body, not in front of the
        // loop (rotation 635; `loop_body_with_hoisted_cond`).
        let (cond, hoisted) = self.parse_loop_clause()?;
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
        let body = Box::new(self.parse_single_stmt_body()?);
        self.reject_decl_in_single_stmt(
            &body,
            body_start,
            "a while loop",
            super::single_stmt_judge::SingleStmtPos::LoopBody,
        )?;
        if hoisted.is_empty() {
            return Ok(Stmt::While { cond, body });
        }
        let tru = self.ast.add_expr(Expr::Bool(true));
        let body = self.loop_body_with_hoisted_cond(hoisted, cond, *body);
        Ok(Stmt::While {
            cond: tru,
            body: Box::new(body),
        })
    }

    pub(super) fn parse_do_while(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `do`
        let body_start = self.pos;
        let body = Box::new(self.parse_single_stmt_body()?);
        self.reject_decl_in_single_stmt(
            &body,
            body_start,
            "a do-while loop",
            super::single_stmt_judge::SingleStmtPos::LoopBody,
        )?;
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
        // Per-iteration position, evaluated AFTER the body — a
        // `continue` must reach it, so a hoisted yield rotates the
        // loop (rotation 636; `rotated_loop`).
        let (cond, hoisted) = self.parse_loop_clause()?;
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
        if hoisted.is_empty() {
            return Ok(Stmt::DoWhile { body, cond });
        }
        let not_cond = self.ast.add_expr(Expr::Unary {
            op: crate::ast::UnaryOp::Not,
            expr: cond,
        });
        let mut head = hoisted;
        head.push(Stmt::If {
            cond: not_cond,
            then_branch: Box::new(Stmt::Break(None)),
            else_branch: None,
        });
        let (init, body) = self.rotated_loop(None, head, None, *body);
        Ok(Stmt::For {
            init,
            cond: None,
            step: None,
            body: Box::new(body),
        })
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
        let scrutinee = self.parse_expr_list()?;
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
        let mut default_at: Option<usize> = None;
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            match self.peek() {
                Token::Case => {
                    self.pos += 1;
                    // Case values evaluate in order until one matches
                    // — conditional position, no yield hoist.
                    let value = self.with_yield_hoist_disallowed(|p| p.parse_expr_list())?;
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
                    default_at = Some(cases.len());
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
            default_at: default_at.unwrap_or(cases.len()),
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
        // A yield in the condition runs per iteration: its hoisted
        // statements go to the top of the body and the clause empties
        // (rotation 635; `loop_body_with_hoisted_cond`). A yield in
        // the step rotates the loop (`rotated_loop`), so `continue`
        // still reaches it.
        let (cond, hoisted) = self.parse_opt_loop_clause(matches!(self.peek(), Token::Semi))?;
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
        // (`for (...; ...; i++, j--)`): §13.16 Expression, the same
        // production the switch scrutinee and the case value spell.
        // Per-iteration position that a `continue` must reach: a
        // hoisted yield rotates the loop (rotation 636;
        // `rotated_loop`).
        let (step, hoisted_step) =
            self.parse_opt_loop_clause(matches!(self.peek(), Token::RParen))?;
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
        let body = Box::new(self.parse_single_stmt_body()?);
        self.reject_decl_in_single_stmt(
            &body,
            body_start,
            "a for loop",
            super::single_stmt_judge::SingleStmtPos::LoopBody,
        )?;
        // A `const` init's binding is immutable in the test, the step
        // and the body (§14.7.4.2 / §9.1.1.1.5; rotation 636).
        let body = if let Some(Stmt::LetDecl {
            mutable: false,
            is_var: false,
            name,
            ..
        }) = init.as_deref()
        {
            let name = name.clone();
            // the operands a yield split off the test or step are
            // statements now
            let mut list: Vec<Stmt> = hoisted.iter().chain(&hoisted_step).cloned().collect();
            list.extend(cond.into_iter().chain(step).map(Stmt::Expr));
            list.push(*body);
            crate::ast::rewrite_self_name_writes(&mut self.ast, &list, &name, true);
            Box::new(list.pop().expect("the body pushed above"))
        } else {
            body
        };
        if !hoisted_step.is_empty() {
            let mut head = hoisted_step;
            head.push(Stmt::Expr(
                step.expect("a hoisted yield came out of a present step"),
            ));
            let (init, body) = self.rotated_loop(init, head, cond.map(|c| (hoisted, c)), *body);
            return Ok(Stmt::For {
                init,
                cond: None,
                step: None,
                body: Box::new(body),
            });
        }
        if hoisted.is_empty() {
            return Ok(Stmt::For {
                init,
                cond,
                step,
                body,
            });
        }
        let cond = cond.expect("a hoisted yield came out of a present condition");
        let body = self.loop_body_with_hoisted_cond(hoisted, cond, *body);
        Ok(Stmt::For {
            init,
            cond: None,
            step,
            body: Box::new(body),
        })
    }
}
