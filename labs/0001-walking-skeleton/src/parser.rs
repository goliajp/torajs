//! Recursive descent parser. Grammar:
//!
//! program  := stmt*
//! stmt     := expr `;`?
//! expr     := additive
//! additive := mul (( `+` | `-` ) mul)*
//! mul      := postfix (( `*` | `/` ) postfix)*
//! postfix  := primary ( `.` ident | `(` args `)` )*
//! args     := (expr (`,` expr)*)?
//! primary  := ident | string | number

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
        if matches!(self.peek(), Token::Let) {
            self.pos += 1;
            let name = match self.peek() {
                Token::Ident(n) => n.clone(),
                t => {
                    return Err(format!(
                        "expected identifier after `let`, got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            self.pos += 1;
            match self.peek() {
                Token::Eq => self.pos += 1,
                t => return Err(format!("expected `=`, got {t:?} at {}", self.at())),
            }
            let init = self.parse_expr()?;
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            return Ok(Stmt::LetDecl { name, init });
        }
        let expr = self.parse_expr()?;
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::Expr(expr))
    }

    fn parse_expr(&mut self) -> Result<ExprId, String> {
        self.parse_additive()
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
            t => Err(format!(
                "expected expression, got {t:?} at {}",
                self.tokens[pos].span.start
            )),
        }
    }
}
