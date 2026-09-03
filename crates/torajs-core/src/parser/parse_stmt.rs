//! `Parser::parse_stmt` extracted from `parser.rs` (chunk 162).
//!
//! Pre-extract this method was 413 LOC inside `impl Parser` block.
//! Body verbatim moves here as an `impl` block sibling — Rust
//! allows multiple impl blocks for the same type, no re-export
//! needed. Follows the pattern of `parser/class_member.rs` +
//! `parser/object_member.rs` already in this directory.
//!
//! `parse_stmt` is the top-level statement dispatcher — peeks the
//! current token and routes to the corresponding parse_* helper
//! (parse_import / parse_export / parse_block / parse_if /
//! parse_fn / parse_class_decl_with_abstract / parse_while /
//! parse_for / try_parse_for_of / parse_return / parse_throw /
//! parse_try / parse_switch / parse_break / parse_continue /
//! parse_let / parse_var / parse_type_decl / parse_expr_stmt).
//! Body unchanged.
//!
//! 2026-07-03 fn-debt decomp: the `yield` statement body and the
//! `let`/`var`/`const` declaration body split into sub-fns
//! `parse_yield_stmt` / `parse_let_decl_stmt` below (bodies
//! verbatim, dedented one level).

use super::*;

impl<'a> Parser<'a> {
    /// Statement dispatcher body — call through the `parse_stmt`
    /// wrapper (yield_expr_hoist.rs), which drains hoisted
    /// expression-position yields in front of the finished statement.
    pub(super) fn parse_stmt_dispatch(&mut self) -> Result<Stmt, String> {
        // V3-18 m1.h.29 — empty statement (`;`). JS spec §13.4
        // ExpressionStatement allows a bare semicolon. Return an
        // empty Block — semantically a no-op, matches what the
        // formatter / lowerer treat as a unit.
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
            return Ok(Stmt::Block(Vec::new()));
        }
        // §16.2 ModuleItem — `import` / `export` DECLARATIONS are
        // reachable only from a module's top level, never from a
        // statement body. `parser/module_item.rs` owns that rule,
        // the ImportCall carve-out, and why a depth counter answers
        // "am I at the top level" without a flag per nesting site.
        if let Some(r) = self.try_parse_module_item() {
            return r;
        }
        if matches!(self.peek(), Token::LBrace) {
            return self.parse_block();
        }
        if matches!(self.peek(), Token::If) {
            return self.parse_if();
        }
        if matches!(self.peek(), Token::While) {
            return self.parse_while();
        }
        if matches!(self.peek(), Token::Do) {
            return self.parse_do_while();
        }
        if matches!(self.peek(), Token::Switch) {
            return self.parse_switch();
        }
        if matches!(self.peek(), Token::For) {
            return self.parse_for();
        }
        if matches!(self.peek(), Token::Break) {
            self.pos += 1;
            let label = self.parse_opt_break_continue_label();
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            return Ok(Stmt::Break(label));
        }
        if matches!(self.peek(), Token::Continue) {
            self.pos += 1;
            let label = self.parse_opt_break_continue_label();
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            return Ok(Stmt::Continue(label));
        }
        if matches!(self.peek(), Token::Function) {
            return self.parse_fn(false);
        }
        // L.2 — `async function f(...)`. The `async` token is consumed
        // and we set is_async on the resulting FnDecl. desugar_async
        // (post-parse) wraps the body's return value in a Promise and
        // shifts the surface return type from T to Promise<T>.
        if matches!(self.peek(), Token::Async) {
            self.pos += 1;
            if !matches!(self.peek(), Token::Function) {
                return Err(format!(
                    "expected `function` after `async`, got {:?} at {}",
                    self.peek(),
                    self.at()
                ));
            }
            return self.parse_fn(true);
        }
        if matches!(self.peek(), Token::Type) {
            return self.parse_type_decl();
        }
        self.judge_with_statement()?;
        if self.at_with_statement() {
            return self.parse_with();
        }
        // V3-18 wedge — `interface X { ... }`. TS-only structural
        // typing declaration; subset desugars to `type X = { ... }`.
        // Contextual keyword: `interface` is just an Ident in the
        // lexer; only treat as a decl when followed by an ident
        // (the interface name).
        if let Token::Ident(s) = self.peek()
            && s == "interface"
            && let Some(next) = self.tokens.get(self.pos + 1)
            && matches!(next.token, Token::Ident(_))
        {
            return self.parse_interface_decl();
        }
        if matches!(self.peek(), Token::Class) {
            return self.parse_class_decl();
        }
        // M-OO.6 — `abstract class C { ... }`. `abstract` is a contextual
        // keyword (just an Ident otherwise) — only treat it as such when
        // followed by `class`.
        if let Token::Ident(s) = self.peek()
            && s == "abstract"
            && let Some(next) = self.tokens.get(self.pos + 1)
            && matches!(next.token, Token::Class)
        {
            self.pos += 1; // consume `abstract`
            let stmt = self.parse_class_decl_with_abstract(true, false, false)?;
            return Ok(self.finish_class_decl_stmt(stmt));
        }
        if matches!(self.peek(), Token::Return) {
            return self.parse_return();
        }
        if matches!(self.peek(), Token::Throw) {
            self.ast.has_try_or_throw = true;
            return self.parse_throw();
        }
        if matches!(self.peek(), Token::Try) {
            self.ast.has_try_or_throw = true;
            return self.parse_try();
        }
        // Outside a generator the token is an identifier candidate
        // (§12.7.2 — strict-goal reservation is judged by the prelude
        // gate), so it falls through to the expression-statement lane
        // and primary.rs admits it.
        if matches!(self.peek(), Token::Yield) && self.in_generator {
            return self.parse_yield_stmt();
        }
        // P2.1 — `var` is parsed identically to `let` here; the
        // difference is the `is_var: true` flag we'll thread into
        // every LetDecl produced from this declaration. The flag
        // drives `desugar_var_hoist` later to lift the declaration
        // to the enclosing fn-body / top-level script (per spec
        // §14.3.2.1 VariableStatement).
        let (mutable, is_var) = match self.peek() {
            // §13.16 — sloppy code can also START a statement with
            // `let` the NAME (`let = let * 2`), so the word only
            // heads a declaration when what follows can begin a
            // binding. `var` has its own token and never asks.
            Token::Let if self.let_begins_declaration() => (Some(true), false),
            Token::Var => (Some(true), true),
            Token::Const => (Some(false), false),
            _ => (None, false),
        };
        if let Some(mutable) = mutable {
            return self.parse_let_decl_stmt(mutable, is_var);
        }
        match self.using_decl_head() {
            Some(false) => return self.parse_using_decl(false),
            Some(true) => {
                // §15.8.1 — `await using` is legal exactly where
                // `await` is (async bodies + module top level; a
                // class static block clears the flag).
                if !self.await_allowed {
                    return Err(format!(
                        "`await` is only valid in async functions and at the top level of a                          module at {} (ES §15.8.1)",
                        self.at()
                    ));
                }
                self.pos += 1; // consume `await`
                return self.parse_using_decl(true);
            }
            None => {}
        }
        if let Some(labeled) = self.try_parse_labeled()? {
            return Ok(labeled);
        }
        let expr = self.parse_expr()?;
        // §13.16 comma-operator expression STATEMENT (`a = 1, b = 2;`
        // / `for (i = 0, j = 9; ...)` init). Every segment's value is
        // discarded, so the segments desugar to sequential statements
        // under a transparent Multi — which also gives each segment
        // the dstr-assign face (`[a] = [1], [b] = [2];`). Comma in
        // expression POSITION (parens, args) is unaffected.
        if matches!(self.peek(), Token::Comma) {
            let mut segs: Vec<Stmt> = vec![self.expr_stmt_or_dstr_assign(expr)?];
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                let e = self.parse_expr()?;
                segs.push(self.expr_stmt_or_dstr_assign(e)?);
            }
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            return Ok(Stmt::Multi(segs));
        }
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        self.expr_stmt_or_dstr_assign(expr)
    }

    /// T-46 — labeled statement (`label: stmt`). JS spec §13.13.
    /// The label is retained (as a `Stmt::Labeled` wrapper) so
    /// `break label` / `continue label` inside `body` can target it.
    /// Stacked labels (`L1: L2: stmt`) nest via the recursive call.
    /// Detection: stmt-level `Ident COLON` is unambiguous — the only
    /// conflicting expression-level shape (`obj: type` in an object
    /// literal / interface) only appears as an Expr context, not as
    /// the first two tokens of a Stmt. `None` = not a label head.
    fn try_parse_labeled(&mut self) -> Result<Option<Stmt>, String> {
        if let Token::Ident(name) = self.peek()
            && let Some(next) = self.tokens.get(self.pos + 1)
            && matches!(next.token, Token::Colon)
        {
            let label = name.clone();
            self.pos += 2; // consume label ident + ':'
            let body_start = self.pos;
            let body = Box::new(self.parse_stmt()?);
            self.reject_decl_in_single_stmt(
                &body,
                body_start,
                "a labeled statement",
                super::single_stmt_judge::SingleStmtPos::LabelledItem,
            )?;
            return Ok(Some(Stmt::Labeled { label, body }));
        }
        Ok(None)
    }

    /// `break`/`continue` optional label — ES §14.9/§14.8 restricted
    /// production `break [no LineTerminator here] LabelIdentifier? ;`.
    /// A newline between the keyword and an identifier triggers ASI, so
    /// `break\n foo` is a bare `break;` followed by the expr-stmt `foo`,
    /// not a labeled break. Caller has already consumed the keyword.
    fn parse_opt_break_continue_label(&mut self) -> Option<String> {
        if let Token::Ident(name) = self.peek()
            && !self.has_newline_before(self.pos)
        {
            let label = name.clone();
            self.pos += 1;
            Some(label)
        } else {
            None
        }
    }
}
