//! `{ … }` block statements and `if` / `else` (§14.2 / §14.6) —
//! carved out of `parser.rs` when the `if ( Expression )` comma-list
//! fix (rotation 629) took the root past the 500-line cap. Pure move;
//! the root keeps the struct, the module list and the class-decl /
//! desugar-id entry points.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_block(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `{`
        let mut stmts = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            stmts.push(self.parse_stmt()?);
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => return Err(format!("expected `}}`, got {t:?} at {}", self.at())),
        }
        Ok(Stmt::Block(stmts))
    }

    pub(super) fn parse_if(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `if`
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `if`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // §14.6 `if ( Expression )` — the comma list.
        let cond = self.parse_expr_list()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after if condition, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let then_start = self.pos;
        let mut then_branch = Box::new(self.parse_stmt()?);
        self.reject_decl_in_single_stmt(
            &then_branch,
            then_start,
            "an if statement",
            single_stmt_judge::SingleStmtPos::IfBranch,
        )?;
        Self::wrap_annexb_if_clause(&mut then_branch);
        let else_branch = if matches!(self.peek(), Token::Else) {
            self.pos += 1;
            let else_start = self.pos;
            let mut e = Box::new(self.parse_stmt()?);
            self.reject_decl_in_single_stmt(
                &e,
                else_start,
                "an else clause",
                single_stmt_judge::SingleStmtPos::IfBranch,
            )?;
            Self::wrap_annexb_if_clause(&mut e);
            Some(e)
        } else {
            None
        };
        Ok(Stmt::If {
            cond,
            then_branch,
            else_branch,
        })
    }
}
