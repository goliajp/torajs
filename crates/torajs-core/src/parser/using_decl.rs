//! `using` declaration heads — Explicit Resource Management (RFC
//! 20260809 B1). `using` lexes as an ordinary Ident, so the head
//! test is shape-based: a BindingIdentifier on the SAME line after
//! the keyword. Everything else keeps its expression reading —
//! `using[i]` / `using(x)` stay member/call expressions, and the
//! line-broken `using \n x` stays an expression statement per ASI.
//! This is exactly the judgment the old loud-reject
//! (`reject_using_decl`) sat on; this file upgrades the sync form to
//! a real parse; knife 2 threads the `await using` head through the
//! same body with `is_await` set (the §15.8.1 context gate lives at
//! the parse_stmt dispatch).

use super::*;

impl<'a> Parser<'a> {
    /// Head test: `Some(true)` = `await using` head, `Some(false)`
    /// = plain `using` head, `None` = not a declaration head (the
    /// expression reading stands).
    pub(super) fn using_decl_head(&self) -> Option<bool> {
        let (using_kw_pos, is_await) = match self.peek() {
            Token::Ident(n) if n == "using" => (self.pos, false),
            Token::Await => match self.tokens.get(self.pos + 1).map(|s| &s.token) {
                Some(Token::Ident(n)) if n == "using" => (self.pos + 1, true),
                _ => return None,
            },
            _ => return None,
        };
        let bind_pos = using_kw_pos + 1;
        (matches!(
            self.tokens.get(bind_pos).map(|s| &s.token),
            Some(Token::Ident(_))
        ) && !self.has_newline_before(bind_pos))
        .then_some(is_await)
    }

    /// Parse `using a = e1, b = e2;` → one `Stmt::UsingDecl` per
    /// binding (single binding stays bare; multiple ride
    /// `Stmt::Multi`, the LetDecl multi-decl convention). Caller has
    /// confirmed a sync `using` head. Spec early errors enforced
    /// here: every binding requires an initializer (§14.3 —
    /// UsingDeclaration has no no-init production); binding patterns
    /// never reach this fn (the head test demands an Ident, so
    /// `using [a] = x` keeps its element-access reading and fails or
    /// succeeds as an expression on its own terms). Only the FIRST
    /// binding carries the no-LineTerminator restriction; later
    /// bindings may wrap freely.
    pub(super) fn parse_using_decl(&mut self, is_await: bool) -> Result<Stmt, String> {
        self.pos += 1; // consume `using`
        let mut decls: Vec<Stmt> = Vec::new();
        loop {
            let name = match self.peek() {
                Token::Ident(n) => n.clone(),
                t => {
                    return Err(format!(
                        "expected identifier after `using`, got {t:?} at {}",
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
            match self.peek() {
                Token::Eq => self.pos += 1,
                t => {
                    return Err(format!(
                        "`using {name}` requires an initializer, got {t:?} at {} (ES2026 UsingDeclaration)",
                        self.at()
                    ));
                }
            }
            let init = self.parse_expr()?;
            decls.push(Stmt::UsingDecl {
                name,
                type_ann,
                init,
                is_await,
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
        Ok(if decls.len() == 1 {
            decls.pop().expect("one decl")
        } else {
            Stmt::Multi(decls)
        })
    }
}
