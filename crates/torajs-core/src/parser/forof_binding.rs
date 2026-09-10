//! for-of loop-variable + destructuring-pattern binding scan, split
//! from `try_parse_for_of` (rotation 119 chunk 6, fn-debt decomp).
//!
//! RFC 20260727-dstr-decl-shape 刀 B — the decl-head pattern scan is
//! the recursive PatShape reader (destr_shape.rs), replacing the
//! names-only lookahead that bailed on defaults / elisions / rest /
//! nesting. The pattern resolves to a `__forof_destr_N` fresh
//! loop-local; the body prepends the same recursive bind expansion
//! the statement form emits. Bare Ident heads and the `var` / bare
//! assign-form wrap are unchanged.

use super::*;

use super::destr_shape::PatShape;

impl<'a> Parser<'a> {
    /// Scan the loop-variable position: destructuring pattern (array
    /// `[a,b]` / object `{x,y}`, recursive) → `__forof_destr_N` fresh
    /// loop-local with a body-prepend of pattern binds; bare Ident →
    /// the user's binding name; `var`/bare head → an `__forvar_N`
    /// fresh loop-local with an assign-form body wrap so the user's
    /// binding tracks each iteration. Returns `None` on a
    /// non-pattern / unrecognised token so the caller can restore
    /// `pos = saved` and return `Ok(None)`.
    pub(super) fn parse_forof_binding_and_pattern(
        &mut self,
        saved: usize,
        is_var_decl: Option<bool>,
        bare_form: bool,
    ) -> Option<(Option<PatShape>, String, Option<String>, Option<ExprId>)> {
        // §14.7.5 — a bare head is a LeftHandSideExpression, not only
        // a name: `for (x.y of …)`, `for (o[k] in …)`, `for ((x) of …)`
        // (the parenthesized cover), `for (this.#p of …)`,
        // `for (super.x in …)`. Parsed under [~In] so the head's own
        // `in` is not taken as the operator. A name (the cover form
        // too) takes the bare-name path below; a member / index
        // target is assigned per iteration from the fresh loop-local
        // (`wrap_assign_expr_form`); a CALL is Annex B §B.3.6.1's
        // sloppy-only AssignmentTargetType `web-compat`: the head is
        // evaluated and the PutValue on a non-reference throws a
        // ReferenceError (test262 callexpression-as-for-{in,of}-lhs
        // — the call runs, its result's valueOf never does); anything
        // else is not a valid assignment target and the for-of
        // attempt is surrendered.
        let ident_head = matches!(self.peek(), Token::Ident(_))
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.token),
                Some(Token::Ident(n)) if n == "of" || n == "in"
            );
        if bare_form && !ident_head && !matches!(self.peek(), Token::LBracket | Token::LBrace) {
            // §14.7.5 ForInOfStatement: `for ( [lookahead ∉ { let, async
            // of }] LeftHandSideExpression of …` — a bare `async` head
            // followed by `of` is a SyntaxError (test262 head-lhs-async-
            // invalid), while `(async) of` (the cover) and `async.x of`
            // are fine. `async` is its own token, so it never took the
            // bare-name path; surrendering here keeps the C-style parse's
            // refusal.
            if matches!(self.peek(), Token::Async)
                && matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.token),
                    Some(Token::Ident(n)) if n == "of"
                )
            {
                self.pos = saved;
                return None;
            }
            let saved_no_in = std::mem::replace(&mut self.in_for_init, true);
            let lhs = self.parse_assign();
            self.in_for_init = saved_no_in;
            let Ok(lhs) = lhs else {
                self.pos = saved;
                return None;
            };
            let id = self.mint_desugar_id();
            return match self.ast.get_expr(lhs) {
                Expr::Ident(n) => Some((None, format!("__forvar_{id}"), Some(n.clone()), None)),
                Expr::Member { .. } | Expr::Index { .. } => {
                    Some((None, format!("__forvar_{id}"), None, Some(lhs)))
                }
                Expr::Call { .. } if !self.strict_here() => {
                    Some((None, format!("__forvar_{id}"), None, Some(lhs)))
                }
                _ => {
                    self.pos = saved;
                    None
                }
            };
        }
        let destruct_pat: Option<PatShape> =
            if matches!(self.peek(), Token::LBracket | Token::LBrace) {
                // `for (var { let } of …)` binds; `for (let { let } of …)`
                // does not. A bare head has no declaration at all, so its
                // names are assignment targets rather than lexical
                // bindings. Restored around the read alone, because the
                // error arm below returns from the middle of it.
                let saved_lex = std::mem::replace(
                    &mut self.pattern_binds_lexically,
                    matches!(is_var_decl, Some(false)),
                );
                let read = self.read_pattern_shape();
                self.pattern_binds_lexically = saved_lex;
                match read {
                    Ok(pat) => Some(pat),
                    // Not a well-formed pattern — this head is a
                    // C-style init that happens to open with `[`/`{`
                    // (or a real syntax error the C-style parse will
                    // report); surrender the for-of attempt.
                    Err(_) => {
                        self.pos = saved;
                        return None;
                    }
                }
            } else {
                None
            };
        let var_name = if destruct_pat.is_some() {
            let id = self.mint_desugar_id();
            format!("__forof_destr_{id}")
        } else {
            // The same admission the pattern head above asks, on the
            // same axis: `for (var let of xs)` binds and
            // `for (let let of xs)` does not (§14.3.1.1). A bare head
            // declares nothing at all.
            let saved_lex = std::mem::replace(
                &mut self.pattern_binds_lexically,
                matches!(is_var_decl, Some(false)),
            );
            let bind = self.pattern_binding_name();
            self.pattern_binds_lexically = saved_lex;
            match bind {
                Some(nn) => {
                    self.pos += 1;
                    nn
                }
                None => {
                    self.pos = saved;
                    return None;
                }
            }
        };
        // chunk B2 — `var` / bare forms route through a fresh
        // loop-local; the user's binding is assigned at the top of
        // each iteration (var+destructuring keeps the block-scoped
        // per-field lets — recorded divergence on fn-scope leak).
        let assign_target: Option<String> =
            if (bare_form || is_var_decl == Some(true)) && destruct_pat.is_none() {
                Some(var_name.clone())
            } else {
                None
            };
        let var_name = if assign_target.is_some() {
            let id = self.mint_desugar_id();
            format!("__forvar_{id}")
        } else {
            var_name
        };
        Some((destruct_pat, var_name, assign_target, None))
    }

    /// Does a depth-0 contextual `of` / `in` close this head before
    /// any `;` — the shape of an expression LHS (`for (x.y of …)`)?
    /// A C-style init meets its `;` first (an `in` there is refused
    /// by [~In] anyway), and a head that ends at `)` is neither. The
    /// token at the cursor itself is excluded, so `for (of of xs)`
    /// keeps its bare-name reading.
    pub(super) fn forof_expr_head_ahead(&self) -> bool {
        let mut depth = 0i32;
        let mut k = self.pos;
        while let Some(t) = self.tokens.get(k) {
            match &t.token {
                Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                Token::RParen | Token::RBracket | Token::RBrace => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                }
                Token::Semi if depth == 0 => return false,
                Token::Ident(n) if depth == 0 && k > self.pos && (n == "of" || n == "in") => {
                    return true;
                }
                _ => {}
            }
            k += 1;
        }
        false
    }

    /// The per-iteration write-back a non-declaration head asks for,
    /// prepended to the body: a bare NAME is assigned from the fresh
    /// loop-local (chunk B2, `var` and bare forms); an expression LHS
    /// is evaluated and assigned there (§14.7.5.7 ForIn/OfBodyEvaluation
    /// step 6.g — the reference lives in the body); a bare assignment
    /// PATTERN is destructured into the existing bindings (S2.24 刀 2,
    /// `desugar_dstr_assign` hoists the local into its own temp, then
    /// assigns per slot). A declaration head writes nothing here.
    pub(super) fn wrap_forof_head_writes(
        &mut self,
        assign_target: &Option<String>,
        lhs_expr: Option<ExprId>,
        destruct_assign: Option<ExprId>,
        var_name: &str,
        body: Stmt,
    ) -> Result<Stmt, String> {
        let body = if let Some(target) = assign_target {
            self.wrap_assign_form(target, var_name, body)
        } else if let Some(lhs) = lhs_expr {
            self.wrap_assign_expr_form(lhs, var_name, body)
        } else {
            body
        };
        if let Some(pat) = destruct_assign {
            let var_ref = self.ast.add_expr(Expr::Ident(var_name.to_string()));
            let mut pre = self.desugar_dstr_assign(pat, var_ref)?;
            pre.push(body);
            return Ok(Stmt::Block(pre));
        }
        Ok(body)
    }

    /// `{ <lhs> = __forvar_N; body }` — the expression-LHS twin of
    /// `wrap_assign_form`: the head's reference is evaluated and
    /// written at the top of every iteration. A CALL head (Annex B,
    /// sloppy) is `{ <call>; throw new ReferenceError(…); body }`:
    /// §14.7.5.7 step 6.g evaluates the LHS, then PutValue on the
    /// non-reference it answers throws (§6.2.5.6 step 3).
    pub(super) fn wrap_assign_expr_form(&mut self, lhs: ExprId, fresh: &str, body: Stmt) -> Stmt {
        if matches!(self.ast.get_expr(lhs), Expr::Call { .. }) {
            let msg = self.ast.add_expr(Expr::String(
                "Invalid left-hand side in for-in/of loop"
                    .to_string()
                    .into(),
            ));
            let err = self.ast.add_expr(Expr::New {
                class_name: "ReferenceError".to_string(),
                args: vec![msg],
                type_args: Vec::new(),
            });
            return Stmt::Block(vec![Stmt::Expr(lhs), Stmt::Throw(err), body]);
        }
        let fresh_ref = self.ast.add_expr(Expr::Ident(fresh.to_string()));
        let assign = self.ast.add_expr(Expr::Assign {
            target: lhs,
            value: fresh_ref,
        });
        Stmt::Block(vec![Stmt::Expr(assign), body])
    }

    /// Prepend the recursive pattern binds when the loop var was a
    /// decl-head pattern — the original `body` is wrapped in a block
    /// so block-close drops still fire normally. Replaces the flat
    /// per-name/per-field wrap (wrap_forof_destr_body).
    pub(super) fn wrap_forof_pattern_body(
        &mut self,
        destruct_pat: &Option<PatShape>,
        var_name: &str,
        body: Stmt,
    ) -> Stmt {
        let Some(pat) = destruct_pat else {
            return body;
        };
        let src_ref = self.ast.add_expr(Expr::Ident(var_name.to_string()));
        let mut pre: Vec<Stmt> = Vec::new();
        self.emit_pattern_binds(pat, src_ref, false, false, &mut pre);
        pre.push(body);
        Stmt::Block(pre)
    }

    /// S2.24 刀 2 (RFC 20260727-dstr-assignment) — bare
    /// assignment-pattern head scan: `for ([a, b] of src)` /
    /// `for ({ x } of src)`. The pattern parses as a literal
    /// expression (the spec's CoverAssignmentPattern, same as the
    /// statement form); a non-of/in follow means this was a C-style
    /// init that happens to open with `[` — the caller restores its
    /// saved position and falls through.
    pub(super) fn scan_forof_assign_pattern(&mut self) -> Option<ExprId> {
        let Ok(pat) = self.parse_expr() else {
            return None;
        };
        let next_is_of_in = matches!(
            self.peek(),
            Token::Ident(n) if n == "of" || n == "in"
        );
        next_is_of_in.then_some(pat)
    }
}
