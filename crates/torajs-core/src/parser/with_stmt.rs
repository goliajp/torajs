//! §14.11 `with ( Expression ) Statement` — the parse half.
//!
//! The statement never reaches the AST as a node of its own. A
//! `Stmt::With` variant would have to be answered by the 93 files that
//! match `Stmt` exhaustively, and rotation 390/391 is a standing
//! lesson about what a new node shape costs the walkers that scan for
//! one: `instanceof`'s right side became an ordinary expression and
//! two whole-program analyses silently changed their minds about
//! unrelated programs.
//!
//! So the parser emits the shape the desugar wants instead:
//!
//! ```text
//! with (E) S   ->   { let __with_<n> = __torajs_with_obj(E); S }
//! ```
//!
//! `ast::desugar_with` runs first in the prelude, recognises a Block
//! whose head is a `__with_`-prefixed binding, and rewrites the free
//! names in `S` against it. Nothing else in the pipeline ever sees a
//! `with` — by the time the second prelude pass runs, this is ordinary
//! code.
//!
//! Only the sloppy goal gets here: `judge_with_statement` refuses the
//! strict spelling before the dispatcher reaches this, and the
//! goal-level refusal for a strict SCRIPT lands in
//! `ast::triage_strict_reserved_idents`, which runs before the
//! prelude.

use super::*;

impl Parser<'_> {
    /// `with` is a ReservedWord in the grammar but reaches the parser
    /// as a plain identifier (the lexer's reserved table serves
    /// escaped spellings only), so the statement is recognised the way
    /// `interface` is: the contextual name plus the one token the form
    /// must have next.
    pub(super) fn at_with_statement(&self) -> bool {
        matches!(self.peek(), Token::Ident(s) if s == "with")
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|n| matches!(n.token, Token::LParen))
    }

    pub(super) fn parse_with(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `with`
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `with`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let obj = self.parse_expr()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after with object, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let body = self.parse_stmt()?;

        // Fresh per site off the arena length — nested `with`s and
        // repeated ones in a body each get their own binding, and the
        // desugar keys on the prefix, not on a counter it would have
        // to keep in sync.
        let name = format!("{}{}", crate::ast::WITH_OBJ_PREFIX, self.ast.exprs.len());
        // §14.11.2 step 2 is ToObject(E), which is a TypeError for
        // null / undefined — `__torajs_with_obj` is that step, not a
        // plain binding.
        let callee = self
            .ast
            .add_expr(Expr::Ident(crate::ast::WITH_OBJ_FN.to_string()));
        let init = self.ast.add_expr(Expr::Call {
            callee,
            args: vec![obj],
        });
        self.ast.has_with_stmt = true;
        Ok(Stmt::Block(vec![
            Stmt::LetDecl {
                mutable: false,
                name,
                type_ann: Some("any".to_string()),
                init,
                is_var: false,
            },
            body,
        ]))
    }
}
