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
        if matches!(self.peek(), Token::Import) {
            return self.parse_import();
        }
        if matches!(self.peek(), Token::Export) {
            return self.parse_export();
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
            return self.parse_class_decl_with_abstract(true, false, false);
        }
        if matches!(self.peek(), Token::Return) {
            return self.parse_return();
        }
        if matches!(self.peek(), Token::Throw) {
            return self.parse_throw();
        }
        if matches!(self.peek(), Token::Try) {
            return self.parse_try();
        }
        if matches!(self.peek(), Token::Yield) {
            return self.parse_yield_stmt();
        }
        // P2.1 — `var` is parsed identically to `let` here; the
        // difference is the `is_var: true` flag we'll thread into
        // every LetDecl produced from this declaration. The flag
        // drives `desugar_var_hoist` later to lift the declaration
        // to the enclosing fn-body / top-level script (per spec
        // §14.3.2.1 VariableStatement).
        let (mutable, is_var) = match self.peek() {
            Token::Let => (Some(true), false),
            Token::Var => (Some(true), true),
            Token::Const => (Some(false), false),
            _ => (None, false),
        };
        if let Some(mutable) = mutable {
            return self.parse_let_decl_stmt(mutable, is_var);
        }
        // T-46 — labeled statement (`label: stmt`). JS spec §13.13.
        // The label is retained (as a `Stmt::Labeled` wrapper) so
        // `break label` / `continue label` inside `body` can target it.
        // Stacked labels (`L1: L2: stmt`) nest via the recursive call.
        // Detection: stmt-level `Ident COLON` is unambiguous — the
        // only conflicting expression-level shape (`obj: type` in an
        // object literal / interface) only appears as an Expr context,
        // not as the first two tokens of a Stmt.
        if let Token::Ident(name) = self.peek()
            && let Some(next) = self.tokens.get(self.pos + 1)
            && matches!(next.token, Token::Colon)
        {
            let label = name.clone();
            self.pos += 2; // consume label ident + ':'
            let body = Box::new(self.parse_stmt()?);
            self.reject_decl_in_single_stmt(&body, "a labeled statement")?;
            return Ok(Stmt::Labeled { label, body });
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

    /// `let` / `var` / `const` declaration statement (multi-decl,
    /// destructuring dispatch, `= yield` J.4 shape) — split from
    /// `parse_stmt` (2026-07-03, fn-debt decomp). Body verbatim,
    /// dedented one level.
    pub(super) fn parse_let_decl_stmt(
        &mut self,
        mutable: bool,
        is_var: bool,
    ) -> Result<Stmt, String> {
        let kw = if is_var {
            "var"
        } else if mutable {
            "let"
        } else {
            "const"
        };
        self.pos += 1;
        // Destructuring: `let [a, b] = src` or `let { x, y } = src`.
        // Parsed inline so it shares the let-decl's lookahead. Both
        // forms desugar to `let __t = src; let <field>...; ...` so the
        // backend never sees a destructuring pattern.
        if matches!(self.peek(), Token::LBracket | Token::LBrace) {
            return self.parse_destructuring_decl(mutable);
        }
        // V3-18 m1.h.5 — multi-decl `let a, b = 1, c` per spec
        // §14.3.1. Each binding can have its own type ann and
        // optional init; commas separate; final semi closes.
        // Decls are emitted as a Stmt::Multi so subsequent
        // passes see them as a flat statement sequence.
        let mut decls: Vec<Stmt> = Vec::new();
        loop {
            let name = match self.peek() {
                Token::Ident(n) => n.clone(),
                t => {
                    return Err(format!(
                        "expected identifier after `{kw}`, got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            self.pos += 1;
            let type_ann = if matches!(self.peek(), Token::Colon) {
                self.pos += 1;
                Some(self.parse_type_ann()?)
            } else {
                None
            };
            // No-init shape: `let x` / `let x: T` (followed by
            // `,` or `;` or — per JS ASI — a known statement-
            // start token on the next line). Const requires an
            // init by spec. T-37-followup-asi: accept Switch /
            // If / For / While / Try / Function / Class / Let /
            // Const / Var / Return / Throw / Break / Continue /
            // Do / RBrace as ASI-implied terminators so test262
            // patterns like `let x\nswitch (x) {...}` parse.
            let next_is_stmt_start = matches!(
                self.peek(),
                Token::Switch
                    | Token::If
                    | Token::For
                    | Token::While
                    | Token::Try
                    | Token::Function
                    | Token::Class
                    | Token::Let
                    | Token::Const
                    | Token::Var
                    | Token::Return
                    | Token::Throw
                    | Token::Break
                    | Token::Continue
                    | Token::Do
                    | Token::RBrace
            );
            if matches!(self.peek(), Token::Semi | Token::Comma) || next_is_stmt_start {
                if !mutable {
                    return Err(format!(
                        "`const {name}` requires an initializer at {}",
                        self.at()
                    ));
                }
                let init = self.ast.add_expr(Expr::Uninit);
                decls.push(Stmt::LetDecl {
                    mutable,
                    name,
                    type_ann,
                    init,
                    is_var,
                });
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                    continue;
                }
                // Only consume Semi as terminator; for ASI-style
                // stmt-start, leave the token for the outer parse.
                if matches!(self.peek(), Token::Semi) {
                    self.pos += 1;
                }
                break;
            }
            match self.peek() {
                Token::Eq => self.pos += 1,
                t => return Err(format!("expected `=`, got {t:?} at {}", self.at())),
            }
            // J.4 — `let name(:T)? = yield <expr>;` shape. Only
            // valid as a single-decl for-loop init or assignment;
            // not allowed in the middle of a multi-decl. If the
            // user writes `let x = yield e, y = ...` we fall
            // through to parse_expr which won't accept yield —
            // matches the v0.5 generator semantics.
            if decls.is_empty() && matches!(self.peek(), Token::Yield) {
                self.pos += 1;
                // S2.41 — `let v = yield;` binds the resumption value
                // with an undefined operand (same optional-operand
                // rule as the statement lane).
                let value = self.parse_yield_operand()?;
                if matches!(self.peek(), Token::Semi) {
                    self.pos += 1;
                }
                return Ok(Stmt::YieldInto {
                    var: name,
                    type_ann,
                    value,
                });
            }
            let init = self.parse_expr()?;
            // P8.5 — narrow-surface class-value alias registration.
            // Peek the init expr:
            //   (i) `const F = class { ... }` → init is the synth
            //       Ident emitted by parse_primary's Class branch
            //       (`__ClassExpr_<id>`). Register F → that name.
            //   (ii) `const G = F` where F is already an alias →
            //        propagate so G also maps to the underlying
            //        synth class.
            // The map is read by parse_new (`new F()` → the synth
            // class's static factory) and by parse_postfix's Dot arm
            // (`F.method(...)` → the synth class's static-method
            // machinery). RC-3 (RFC 20260706-test262-bug-corpus):
            // let/var bindings register too — the map is linear
            // parse-order (not scoped), so any later rebinding or
            // reassignment of the name drops the alias and falls
            // back to the dynamic path instead of silently binding
            // the old class.
            {
                let mut aliased = false;
                if let Expr::Ident(init_name) = self.ast.get_expr(init) {
                    if init_name.starts_with("__ClassExpr_") {
                        self.class_value_aliases
                            .insert(name.clone(), init_name.clone());
                        // RFC 20260714-dstr-residual blade 4 — ES
                        // §8.4.5 NamedEvaluation: `let D = class {}`
                        // names the anonymous class expression by its
                        // binding (first binding wins; a later alias
                        // of the same synth class doesn't rename it).
                        let init_name = init_name.clone();
                        self.ast
                            .class_expr_display_names
                            .entry(init_name)
                            .or_insert_with(|| name.clone());
                        aliased = true;
                    } else if let Some(target) = self.class_value_aliases.get(init_name) {
                        let target = target.clone();
                        self.class_value_aliases.insert(name.clone(), target);
                        aliased = true;
                    }
                }
                if !aliased {
                    self.class_value_aliases.remove(&name);
                }
            }
            decls.push(Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
                is_var,
            });
            if matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                continue;
            }
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            break;
        }
        return Ok(if decls.len() == 1 {
            decls.into_iter().next().unwrap()
        } else {
            Stmt::Multi(decls)
        });
    }
}
