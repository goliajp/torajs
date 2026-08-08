//! `for (using x of …)` / `for (await using x of …)` head support —
//! RFC 20260809 knife 3, split from `try_parse_for_of.rs` (the head
//! scan + body wrap pushed the host over the 500-line hard limit).
//!
//! The head is shape-tested (contextual `using`, optionally behind
//! `await`, then a binding Ident, then `of`/`in`): `for (using of
//! …)` keeps `using` as the bare binding and `using[i]` never
//! reaches the test — `[` is not an Ident. The declaration behaves
//! like let/const at the loop layer; the BODY opens with `using x =
//! <fresh>` so the prelude desugar builds the per-iteration env +
//! try/finally — the spec's dispose-at-end-of-each-iteration timing
//! (break / continue / throw included) rides the block-exit shape.

use super::*;

impl<'a> Parser<'a> {
    /// The two head-misuse early errors, folded into one call so the
    /// host fn stays under the size line: `kind: None` = the §15.8.1
    /// await-context gate (an `await using` head outside an async
    /// body / module top level); `kind: Some("in")` = §14.7.5, the
    /// for-IN head grammar has no using form.
    pub(super) fn reject_forof_using_misuse(
        &self,
        is_using: Option<bool>,
        kind: Option<&str>,
    ) -> Result<(), String> {
        match kind {
            None if is_using == Some(true) && !self.await_allowed => Err(format!(
                "`await` is only valid in async functions and at the top level of a \
                 module at {} (ES §15.8.1)",
                self.at()
            )),
            Some("in") if is_using.is_some() => Err(format!(
                "`using` bindings are not allowed in a for-in head at {}",
                self.at()
            )),
            _ => Ok(()),
        }
    }

    /// Head shape test. `Some(is_await)` = a using head was consumed
    /// (cursor sits on the binding Ident); `None` = not a using head
    /// (cursor untouched).
    pub(super) fn scan_forof_using_head(&mut self) -> Option<bool> {
        let (u_at, is_await) = match self.peek() {
            Token::Ident(u) if u == "using" => (self.pos, false),
            Token::Await
                if matches!(self.tokens.get(self.pos + 1).map(|t| &t.token),
                    Some(Token::Ident(n)) if n == "using") =>
            {
                (self.pos + 1, true)
            }
            _ => return None,
        };
        (matches!(self.tokens.get(u_at + 1).map(|t| &t.token), Some(Token::Ident(n)) if n != "of" && n != "in")
            && matches!(self.tokens.get(u_at + 2).map(|t| &t.token), Some(Token::Ident(n)) if n == "of" || n == "in"))
        .then(|| {
            self.pos = u_at + 1; // past `await`? + `using`
            is_await
        })
    }

    /// Body wrap: mint the fresh loop local and open the body with
    /// `using <user> = <fresh>` carrying the dispose hint.
    pub(super) fn wrap_forof_using_body(
        &mut self,
        user_var: String,
        body: Stmt,
        hint_await: bool,
    ) -> (String, Stmt) {
        let id = self.mint_desugar_id();
        let fresh = format!("__fouse_{id}");
        let fresh_ref = self.ast.add_expr(Expr::Ident(fresh.clone()));
        let user_decl = Stmt::UsingDecl {
            name: user_var,
            type_ann: None,
            init: fresh_ref,
            is_await: hint_await,
        };
        (fresh, Stmt::Block(vec![user_decl, body]))
    }
}
