//! Recursive descent parser. Grammar:
//!
//! program  := stmt*
//! stmt     := decl | if | while | block | expr `;`?
//! decl     := (`let` | `const`) IDENT (`:` IDENT)? `=` expr `;`?
//! if       := `if` `(` expr `)` stmt (`else` stmt)?
//! while    := `while` `(` expr `)` stmt
//! block    := `{` stmt* `}`
//! expr     := assign
//! assign   := equality (`=` assign)?               (* right-associative *)
//! equality := comparison ((`===` | `!==`) comparison)*
//! comparison := additive ((`<`|`>`|`<=`|`>=`) additive)*
//! additive := mul (( `+` | `-` ) mul)*
//! mul      := postfix (( `*` | `/` ) postfix)*
//! postfix  := primary ( `.` ident | `(` args `)` )*
//! args     := (expr (`,` expr)*)?
//! primary  := ident | string | number | `true` | `false`

use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt};
use crate::lexer::{Spanned, Token};

pub fn parse(tokens: &[Spanned]) -> Result<Ast, String> {
    let mut p = Parser {
        tokens,
        pos: 0,
        ast: Ast::default(),
    };
    p.parse_program()?;
    Ok(p.ast)
}

struct Parser<'a> {
    tokens: &'a [Spanned],
    pos: usize,
    ast: Ast,
}

impl Parser<'_> {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn at(&self) -> u32 {
        self.tokens[self.pos].span.start
    }

    fn parse_program(&mut self) -> Result<(), String> {
        while !matches!(self.peek(), Token::Eof) {
            let stmt = self.parse_stmt()?;
            self.ast.stmts.push(stmt);
        }
        Ok(())
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        if matches!(self.peek(), Token::LBrace) {
            return self.parse_block();
        }
        if matches!(self.peek(), Token::If) {
            return self.parse_if();
        }
        if matches!(self.peek(), Token::While) {
            return self.parse_while();
        }
        let mutable = match self.peek() {
            Token::Let => Some(true),
            Token::Const => Some(false),
            _ => None,
        };
        if let Some(mutable) = mutable {
            let kw = if mutable { "let" } else { "const" };
            self.pos += 1;
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
                let ann = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    t => {
                        return Err(format!(
                            "expected type name after `:`, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                Some(ann)
            } else {
                None
            };
            match self.peek() {
                Token::Eq => self.pos += 1,
                t => return Err(format!("expected `=`, got {t:?} at {}", self.at())),
            }
            let init = self.parse_expr()?;
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            return Ok(Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
            });
        }
        let expr = self.parse_expr()?;
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::Expr(expr))
    }

    fn parse_block(&mut self) -> Result<Stmt, String> {
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

    fn parse_if(&mut self) -> Result<Stmt, String> {
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
        let cond = self.parse_expr()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after if condition, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let then_branch = Box::new(self.parse_stmt()?);
        let else_branch = if matches!(self.peek(), Token::Else) {
            self.pos += 1;
            Some(Box::new(self.parse_stmt()?))
        } else {
            None
        };
        Ok(Stmt::If {
            cond,
            then_branch,
            else_branch,
        })
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
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
        let cond = self.parse_expr()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after while condition, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::While { cond, body })
    }

    fn parse_expr(&mut self) -> Result<ExprId, String> {
        self.parse_assign()
    }

    fn parse_assign(&mut self) -> Result<ExprId, String> {
        let target = self.parse_equality()?;
        if matches!(self.peek(), Token::Eq) {
            self.pos += 1;
            let value = self.parse_assign()?; // right-associative
            return Ok(self.ast.add_expr(Expr::Assign { target, value }));
        }
        Ok(target)
    }

    fn parse_equality(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_comparison()?;
        loop {
            let op = match self.peek() {
                Token::EqEqEq => BinOp::Eq,
                Token::BangEqEq => BinOp::Neq,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.parse_comparison()?;
            left = self.ast.add_expr(Expr::BinOp { op, left, right });
        }
    }

    fn parse_comparison(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                Token::Lt => BinOp::Lt,
                Token::Gt => BinOp::Gt,
                Token::LtEq => BinOp::Le,
                Token::GtEq => BinOp::Ge,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.parse_additive()?;
            left = self.ast.add_expr(Expr::BinOp { op, left, right });
        }
    }

    fn parse_additive(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.parse_multiplicative()?;
            left = self.ast.add_expr(Expr::BinOp { op, left, right });
        }
    }

    fn parse_multiplicative(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_postfix()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.parse_postfix()?;
            left = self.ast.add_expr(Expr::BinOp { op, left, right });
        }
    }

    fn parse_postfix(&mut self) -> Result<ExprId, String> {
        let mut node = self.parse_primary()?;
        loop {
            match self.peek() {
                Token::Dot => {
                    self.pos += 1;
                    let name = match self.peek() {
                        Token::Ident(n) => n.clone(),
                        t => {
                            return Err(format!(
                                "expected identifier after `.`, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    };
                    self.pos += 1;
                    node = self.ast.add_expr(Expr::Member { obj: node, name });
                }
                Token::LParen => {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        args.push(self.parse_expr()?);
                        while matches!(self.peek(), Token::Comma) {
                            self.pos += 1;
                            args.push(self.parse_expr()?);
                        }
                    }
                    match self.peek() {
                        Token::RParen => self.pos += 1,
                        t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
                    }
                    node = self.ast.add_expr(Expr::Call { callee: node, args });
                }
                _ => return Ok(node),
            }
        }
    }

    fn parse_primary(&mut self) -> Result<ExprId, String> {
        let pos = self.pos;
        match &self.tokens[pos].token {
            Token::Ident(n) => {
                let n = n.clone();
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Ident(n)))
            }
            Token::String(s) => {
                let s = s.clone();
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::String(s)))
            }
            Token::Number(n) => {
                let n = *n;
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Number(n)))
            }
            Token::True => {
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Bool(true)))
            }
            Token::False => {
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Bool(false)))
            }
            t => Err(format!(
                "expected expression, got {t:?} at {}",
                self.tokens[pos].span.start
            )),
        }
    }
}
