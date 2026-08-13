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
    ) -> Option<(Option<PatShape>, String, Option<String>)> {
        let destruct_pat: Option<PatShape> =
            if matches!(self.peek(), Token::LBracket | Token::LBrace) {
                match self.read_pattern_shape() {
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
            match self.peek() {
                Token::Ident(n) => {
                    let nn = n.clone();
                    self.pos += 1;
                    nn
                }
                _ => {
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
        Some((destruct_pat, var_name, assign_target))
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
