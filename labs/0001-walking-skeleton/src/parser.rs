//! Recursive descent parser. Grammar:
//!
//! program  := stmt*
//! stmt     := decl | if | while | block | fndecl | return | expr `;`?
//! decl     := (`let` | `const`) IDENT (`:` IDENT)? `=` expr `;`?
//! if       := `if` `(` expr `)` stmt (`else` stmt)?
//! while    := `while` `(` expr `)` stmt
//! block    := `{` stmt* `}`
//! fndecl   := `function` IDENT `(` params? `)` (`:` IDENT)? `{` stmt* `}`
//! params   := param (`,` param)*
//! param    := IDENT (`:` IDENT)?
//! return   := `return` expr? `;`?
//! expr     := assign
//! assign   := lor (`=` assign)?                    (* right-associative *)
//! lor      := land (`||` land)*
//! land     := bit_or (`&&` bit_or)*
//! bit_or   := bit_xor (`|` bit_xor)*
//! bit_xor  := bit_and (`^` bit_and)*
//! bit_and  := equality (`&` equality)*
//! equality := comparison ((`===` | `!==`) comparison)*
//! comparison := shift ((`<`|`>`|`<=`|`>=`) shift)*
//! shift    := additive ((`<<`|`>>`) additive)*
//! additive := mul (( `+` | `-` ) mul)*
//! mul      := unary (( `*` | `/` | `%` ) unary)*
//! unary    := `!` unary | postfix
//! postfix  := primary ( `.` ident | `(` args `)` | `[` expr `]` )*
//! args     := (expr (`,` expr)*)?
//! primary  := ident | string | number | `true` | `false` | arrow_fn | array_lit
//! arrow_fn := `(` params? `)` (`:` IDENT)? `=>` (block | expr)
//! array_lit := `[` (expr (`,` expr)*)? `]`
//! type_ann := IDENT (`[` `]`)*

use crate::ast::{self, Ast, BinOp, Expr, ExprId, Param, Stmt};
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

    /// Parse a type annotation. Supports IDENT, array suffixes (`T[]`,
    /// `T[][]`), and function types (`(p1: T1, p2: T2) => R`). Returns the
    /// annotation as a flat string. Encoding for fn types: `__fn(T1|T2)->R`
    /// (param types separated by `|`, return after `->`). Param names are
    /// parsed but discarded — only types matter at the type-system level,
    /// matching TS's own treatment of fn-type annotations.
    ///
    /// M2 Phase B Stage 1.
    fn parse_type_ann(&mut self) -> Result<String, String> {
        // Function type: `(p: T, ...) => R`.
        if matches!(self.peek(), Token::LParen) {
            return self.parse_fn_type_ann();
        }
        let mut name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!("expected type name, got {t:?} at {}", self.at()));
            }
        };
        self.pos += 1;
        // M3.4 — generic type instantiation `Pair<A, B>`. Encoded into the
        // flat ann string as `Pair<A|B>` (inner `|` mirrors the `__fn(P|Q)`
        // separator). Same depth-aware decoding shape, so check.rs and
        // ssa_lower can share parsers with the existing fn-type reader.
        if matches!(self.peek(), Token::Lt) {
            self.pos += 1;
            let mut args: Vec<String> = Vec::new();
            if !matches!(self.peek(), Token::Gt) {
                loop {
                    args.push(self.parse_type_ann()?);
                    match self.peek() {
                        Token::Comma => self.pos += 1,
                        Token::Gt => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `>` in type args, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
            }
            match self.peek() {
                Token::Gt => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `>` to close type args, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            name = format!("{name}<{}>", args.join("|"));
        }
        while matches!(self.peek(), Token::LBracket) {
            self.pos += 1;
            match self.peek() {
                Token::RBracket => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `]` in array type, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            name.push_str("[]");
        }
        Ok(name)
    }

    fn parse_fn_type_ann(&mut self) -> Result<String, String> {
        // current token = `(`
        self.pos += 1;
        let mut params: Vec<String> = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                // Optional `name:` prefix on each param. Name is discarded;
                // we keep only the type. Two shapes accepted:
                //   `name: T` — TS standard fn-type form.
                //   `T`       — bare type, no name (fallback).
                let name_then_colon = matches!(self.peek(), Token::Ident(_))
                    && matches!(
                        self.tokens.get(self.pos + 1).map(|s| &s.token),
                        Some(Token::Colon)
                    );
                if name_then_colon {
                    self.pos += 2;
                }
                let pty = self.parse_type_ann()?;
                params.push(pty);
                match self.peek() {
                    Token::Comma => self.pos += 1,
                    Token::RParen => break,
                    t => {
                        return Err(format!(
                            "expected `,` or `)` in fn-type params, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
            }
        }
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
        }
        match self.peek() {
            Token::FatArrow => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `=>` in fn-type, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let ret = self.parse_type_ann()?;
        Ok(format!("__fn({})->{}", params.join("|"), ret))
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
        if matches!(self.peek(), Token::For) {
            return self.parse_for();
        }
        if matches!(self.peek(), Token::Break) {
            self.pos += 1;
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            return Ok(Stmt::Break);
        }
        if matches!(self.peek(), Token::Continue) {
            self.pos += 1;
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            return Ok(Stmt::Continue);
        }
        if matches!(self.peek(), Token::Function) {
            return self.parse_fn();
        }
        if matches!(self.peek(), Token::Type) {
            return self.parse_type_decl();
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
                Some(self.parse_type_ann()?)
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

    fn parse_fn(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `function`
        let name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!(
                    "expected function name, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        // M3 — optional type-parameter list: `function id<T, U>(...)`. If
        // present, the names are recorded on the FnDecl and the typechecker
        // treats them as placeholder types that don't resolve to a concrete
        // shape until each call site.
        let mut type_params: Vec<String> = Vec::new();
        if matches!(self.peek(), Token::Lt) {
            self.pos += 1;
            if !matches!(self.peek(), Token::Gt) {
                loop {
                    match self.peek() {
                        Token::Ident(n) => {
                            type_params.push(n.clone());
                            self.pos += 1;
                        }
                        t => {
                            return Err(format!(
                                "expected type-parameter name, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    match self.peek() {
                        Token::Comma => self.pos += 1,
                        Token::Gt => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `>` in type parameters, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
            }
            match self.peek() {
                Token::Gt => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `>` to close type parameters, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => return Err(format!("expected `(`, got {t:?} at {}", self.at())),
        }
        let mut params = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                let pname = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    t => {
                        return Err(format!(
                            "expected parameter name, got {t:?} at {}",
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
                params.push(Param {
                    name: pname,
                    type_ann,
                });
                match self.peek() {
                    Token::Comma => self.pos += 1,
                    Token::RParen => break,
                    t => return Err(format!("expected `,` or `)`, got {t:?} at {}", self.at())),
                }
            }
        }
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
        }
        let return_type = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            Some(self.parse_type_ann()?)
        } else {
            None
        };
        // body must be a block
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` (function body), got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            body.push(self.parse_stmt()?);
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => return Err(format!("expected `}}`, got {t:?} at {}", self.at())),
        }
        Ok(Stmt::FnDecl {
            name,
            type_params,
            params,
            return_type,
            body,
        })
    }

    fn parse_return(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `return`
        let expr = match self.peek() {
            Token::Semi | Token::RBrace | Token::Eof => None,
            _ => Some(self.parse_expr()?),
        };
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::Return(expr))
    }

    fn parse_throw(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `throw`
        let expr = self.parse_expr()?;
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::Throw(expr))
    }

    /// `try { body } catch (e) { catch_body } [finally { finally_body }]`.
    /// `catch (e)` is required for now — TS allows `try { } finally { }`
    /// without catch but our M4.1 surface requires the catch.
    fn parse_try(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `try`
        let body = self.parse_block_stmts("try")?;
        // TS allows `try { } catch { }` OR `try { } finally { }` OR
        // `try { } catch { } finally { }` — at least one of catch /
        // finally is required.
        let mut catch_param: Option<String> = None;
        let mut catch_type: Option<String> = None;
        let mut catch_body: Vec<Stmt> = Vec::new();
        let mut had_catch = false;
        if matches!(self.peek(), Token::Catch) {
            had_catch = true;
            self.pos += 1;
            // `catch (e[: T])` is optional since ES2019.
            if matches!(self.peek(), Token::LParen) {
                self.pos += 1;
                let n = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    t => {
                        return Err(format!(
                            "expected catch parameter name, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                let ty = if matches!(self.peek(), Token::Colon) {
                    self.pos += 1;
                    Some(self.parse_type_ann()?)
                } else {
                    None
                };
                match self.peek() {
                    Token::RParen => self.pos += 1,
                    t => {
                        return Err(format!(
                            "expected `)` after catch param, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
                catch_param = Some(n);
                catch_type = ty;
            }
            catch_body = self.parse_block_stmts("catch")?;
        }
        let finally_body = if matches!(self.peek(), Token::Finally) {
            self.pos += 1;
            Some(self.parse_block_stmts("finally")?)
        } else {
            None
        };
        if !had_catch && finally_body.is_none() {
            return Err(format!(
                "try block needs `catch` or `finally` (or both); got {:?} at {}",
                self.peek(),
                self.at()
            ));
        }
        Ok(Stmt::Try {
            body,
            catch_param,
            catch_type,
            catch_body,
            finally_body,
        })
    }

    /// Parse a `{ ... }` block as a flat list of statements (used by try /
    /// catch / finally where we want the inner stmts directly, not wrapped
    /// in `Stmt::Block`).
    fn parse_block_stmts(&mut self, ctx: &str) -> Result<Vec<Stmt>, String> {
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` to begin {ctx} block, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut stmts = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            stmts.push(self.parse_stmt()?);
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end {ctx} block, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(stmts)
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

    /// `for (init?; cond?; step?) body`. Each clause is optional but the
    /// two `;` separators are required (matches TS / C). Init is parsed
    /// as a stmt (typically a `let` decl or expr-stmt). Cond is an expr.
    /// Step is an expr (we don't have post-increment yet — use
    /// `i = i + 1`).
    fn parse_for(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `for`
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `for`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // init clause — empty (just `;`) or any stmt that ends with `;`.
        let init = if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
            None
        } else {
            // parse_stmt eats its own trailing `;` for let / expr stmts.
            Some(Box::new(self.parse_stmt()?))
        };
        // cond clause — empty means infinite-loop (true). Empty is `;`.
        let cond = if matches!(self.peek(), Token::Semi) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        match self.peek() {
            Token::Semi => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `;` after `for` condition, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // step clause — empty means no step.
        let step = if matches!(self.peek(), Token::RParen) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after `for` step, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::For { init, cond, step, body })
    }

    fn parse_expr(&mut self) -> Result<ExprId, String> {
        self.parse_assign()
    }

    fn parse_assign(&mut self) -> Result<ExprId, String> {
        let target = self.parse_logical_or()?;
        if matches!(self.peek(), Token::Eq) {
            self.pos += 1;
            let value = self.parse_assign()?; // right-associative
            return Ok(self.ast.add_expr(Expr::Assign { target, value }));
        }
        Ok(target)
    }

    fn parse_logical_or(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_logical_and()?;
        while matches!(self.peek(), Token::PipePipe) {
            self.pos += 1;
            let right = self.parse_logical_and()?;
            left = self.ast.add_expr(Expr::BinOp {
                op: BinOp::LOr,
                left,
                right,
            });
        }
        Ok(left)
    }

    fn parse_logical_and(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_bit_or()?;
        while matches!(self.peek(), Token::AmpAmp) {
            self.pos += 1;
            let right = self.parse_bit_or()?;
            left = self.ast.add_expr(Expr::BinOp {
                op: BinOp::LAnd,
                left,
                right,
            });
        }
        Ok(left)
    }

    fn parse_bit_or(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_bit_xor()?;
        while matches!(self.peek(), Token::Pipe) {
            self.pos += 1;
            let right = self.parse_bit_xor()?;
            left = self.ast.add_expr(Expr::BinOp {
                op: BinOp::BitOr,
                left,
                right,
            });
        }
        Ok(left)
    }

    fn parse_bit_xor(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_bit_and()?;
        while matches!(self.peek(), Token::Caret) {
            self.pos += 1;
            let right = self.parse_bit_and()?;
            left = self.ast.add_expr(Expr::BinOp {
                op: BinOp::BitXor,
                left,
                right,
            });
        }
        Ok(left)
    }

    fn parse_bit_and(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_equality()?;
        while matches!(self.peek(), Token::Amp) {
            self.pos += 1;
            let right = self.parse_equality()?;
            left = self.ast.add_expr(Expr::BinOp {
                op: BinOp::BitAnd,
                left,
                right,
            });
        }
        Ok(left)
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
        let mut left = self.parse_shift()?;
        loop {
            let op = match self.peek() {
                Token::Lt => BinOp::Lt,
                Token::Gt => BinOp::Gt,
                Token::LtEq => BinOp::Le,
                Token::GtEq => BinOp::Ge,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.parse_shift()?;
            left = self.ast.add_expr(Expr::BinOp { op, left, right });
        }
    }

    fn parse_shift(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                Token::ShlShl => BinOp::Shl,
                Token::ShrShr => BinOp::Shr,
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
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.parse_unary()?;
            left = self.ast.add_expr(Expr::BinOp { op, left, right });
        }
    }

    fn parse_unary(&mut self) -> Result<ExprId, String> {
        if matches!(self.peek(), Token::Bang) {
            self.pos += 1;
            // Right-associative: `!!a` = !(!a).
            let inner = self.parse_unary()?;
            return Ok(self.ast.add_expr(Expr::Unary {
                op: ast::UnaryOp::Not,
                expr: inner,
            }));
        }
        if matches!(self.peek(), Token::Minus) {
            self.pos += 1;
            let inner = self.parse_unary()?;
            return Ok(self.ast.add_expr(Expr::Unary {
                op: ast::UnaryOp::Neg,
                expr: inner,
            }));
        }
        self.parse_postfix()
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
                Token::LBracket => {
                    self.pos += 1;
                    let index = self.parse_expr()?;
                    match self.peek() {
                        Token::RBracket => self.pos += 1,
                        t => return Err(format!("expected `]`, got {t:?} at {}", self.at())),
                    }
                    node = self.ast.add_expr(Expr::Index { obj: node, index });
                }
                _ => return Ok(node),
            }
        }
    }

    fn parse_primary(&mut self) -> Result<ExprId, String> {
        if matches!(self.peek(), Token::LParen) {
            // `(` could start either a parenthesized expression `(e)` or an
            // arrow fn `(params) =>`. Disambiguate by scanning forward to the
            // matching `)` and peeking for `=>`.
            if self.is_arrow_fn_at_lparen() {
                return self.parse_arrow_fn();
            }
            self.pos += 1; // consume `(`
            let e = self.parse_expr()?;
            match self.peek() {
                Token::RParen => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `)` after parenthesized expression, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            return Ok(e);
        }
        if matches!(self.peek(), Token::LBracket) {
            return self.parse_array_literal();
        }
        if matches!(self.peek(), Token::LBrace) {
            // `{` in expression position is an object literal. Block
            // statements are caught by `parse_stmt`'s LBrace check before
            // reaching here, so the only path that lands at LBrace in
            // primary is an expression context (let-init, fn arg, return
            // value, etc.).
            return self.parse_object_literal();
        }
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

    /// Lookahead: from a `(` at `self.pos`, find the matching `)` and peek
    /// for `=>` (or `: T => ...`) to decide arrow-fn vs parenthesized expression.
    /// Handles nested parens correctly.
    fn is_arrow_fn_at_lparen(&self) -> bool {
        debug_assert!(matches!(self.peek(), Token::LParen));
        let mut depth: i32 = 1;
        let mut i = self.pos + 1;
        while i < self.tokens.len() {
            match &self.tokens[i].token {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        // Direct arrow: `() => ...`
                        if matches!(
                            self.tokens.get(i + 1).map(|s| &s.token),
                            Some(Token::FatArrow)
                        ) {
                            return true;
                        }
                        // Arrow with explicit return type: `() : T => ...`
                        // — skip past the `: TypeAnnotation` and look for
                        // `=>`. Type annotation is IDENT followed by
                        // optional `<...>` generics + `[]` array suffixes.
                        if matches!(
                            self.tokens.get(i + 1).map(|s| &s.token),
                            Some(Token::Colon)
                        ) {
                            // Scan past the type ann to look for `=>`.
                            let mut j = i + 2;
                            // Type starts with an identifier.
                            if !matches!(
                                self.tokens.get(j).map(|s| &s.token),
                                Some(Token::Ident(_))
                            ) {
                                return false;
                            }
                            j += 1;
                            // Optional generic args `<T1, T2, ...>` —
                            // very rough scan; if we hit a stray `>`
                            // before the next plausible `=>`, fall back
                            // to "not arrow".
                            if matches!(
                                self.tokens.get(j).map(|s| &s.token),
                                Some(Token::Lt)
                            ) {
                                let mut g = 1;
                                j += 1;
                                while j < self.tokens.len() && g > 0 {
                                    match self.tokens[j].token {
                                        Token::Lt => g += 1,
                                        Token::Gt => g -= 1,
                                        Token::ShrShr => g -= 2,
                                        Token::Eof => return false,
                                        _ => {}
                                    }
                                    j += 1;
                                }
                            }
                            // Optional `[]` array suffixes.
                            while matches!(
                                self.tokens.get(j).map(|s| &s.token),
                                Some(Token::LBracket)
                            ) && matches!(
                                self.tokens.get(j + 1).map(|s| &s.token),
                                Some(Token::RBracket)
                            ) {
                                j += 2;
                            }
                            return matches!(
                                self.tokens.get(j).map(|s| &s.token),
                                Some(Token::FatArrow)
                            );
                        }
                        return false;
                    }
                }
                Token::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// `{ name: expr, ... }` — assumes current token is `{`.
    fn parse_object_literal(&mut self) -> Result<ExprId, String> {
        self.pos += 1; // consume `{`
        let mut fields: Vec<(String, ExprId)> = Vec::new();
        if !matches!(self.peek(), Token::RBrace) {
            fields.push(self.parse_object_field()?);
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                if matches!(self.peek(), Token::RBrace) {
                    break; // trailing comma
                }
                fields.push(self.parse_object_field()?);
            }
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` in object literal, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(self.ast.add_expr(Expr::ObjectLit { fields }))
    }

    /// One `name: expr` pair inside an object literal.
    fn parse_object_field(&mut self) -> Result<(String, ExprId), String> {
        let name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!(
                    "expected field name in object literal, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        match self.peek() {
            Token::Colon => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `:` after field name `{name}`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let value = self.parse_expr()?;
        Ok((name, value))
    }

    /// `type Name = { f1: T1, f2: T2 };`
    fn parse_type_decl(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `type`
        let name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!(
                    "expected type name after `type`, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        // M3.4 — optional type parameters: `type Pair<A, B> = { ... }`.
        let mut type_params: Vec<String> = Vec::new();
        if matches!(self.peek(), Token::Lt) {
            self.pos += 1;
            if !matches!(self.peek(), Token::Gt) {
                loop {
                    match self.peek() {
                        Token::Ident(n) => {
                            type_params.push(n.clone());
                            self.pos += 1;
                        }
                        t => {
                            return Err(format!(
                                "expected type-parameter name in type<...>, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    match self.peek() {
                        Token::Comma => self.pos += 1,
                        Token::Gt => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `>` in type params, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
            }
            match self.peek() {
                Token::Gt => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `>` to close type params, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        match self.peek() {
            Token::Eq => self.pos += 1,
            t => return Err(format!("expected `=` after type name, got {t:?} at {}", self.at())),
        }
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` to begin type body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut fields: Vec<(String, String)> = Vec::new();
        if !matches!(self.peek(), Token::RBrace) {
            fields.push(self.parse_type_decl_field()?);
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                if matches!(self.peek(), Token::RBrace) {
                    break;
                }
                fields.push(self.parse_type_decl_field()?);
            }
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end type body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::TypeDecl {
            name,
            type_params,
            fields,
        })
    }

    fn parse_type_decl_field(&mut self) -> Result<(String, String), String> {
        let name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!(
                    "expected field name in type body, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        match self.peek() {
            Token::Colon => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `:` after field name `{name}`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let ty = self.parse_type_ann()?;
        Ok((name, ty))
    }

    fn parse_array_literal(&mut self) -> Result<ExprId, String> {
        // assumes current token is `[`
        self.pos += 1;
        let mut elements = Vec::new();
        if !matches!(self.peek(), Token::RBracket) {
            elements.push(self.parse_expr()?);
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                if matches!(self.peek(), Token::RBracket) {
                    break; // trailing comma allowed
                }
                elements.push(self.parse_expr()?);
            }
        }
        match self.peek() {
            Token::RBracket => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `]` in array literal, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(self.ast.add_expr(Expr::Array(elements)))
    }

    fn parse_arrow_fn(&mut self) -> Result<ExprId, String> {
        // assumes current token is `(`
        self.pos += 1;
        let mut params = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                let pname = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    t => {
                        return Err(format!(
                            "expected parameter name, got {t:?} at {}",
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
                params.push(Param {
                    name: pname,
                    type_ann,
                });
                match self.peek() {
                    Token::Comma => self.pos += 1,
                    Token::RParen => break,
                    t => return Err(format!("expected `,` or `)`, got {t:?} at {}", self.at())),
                }
            }
        }
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
        }
        let return_type = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            Some(self.parse_type_ann()?)
        } else {
            None
        };
        match self.peek() {
            Token::FatArrow => self.pos += 1,
            t => return Err(format!("expected `=>`, got {t:?} at {}", self.at())),
        }
        let body = if matches!(self.peek(), Token::LBrace) {
            self.pos += 1;
            let mut stmts = Vec::new();
            while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                stmts.push(self.parse_stmt()?);
            }
            match self.peek() {
                Token::RBrace => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `}}` after arrow fn body, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            stmts
        } else {
            // expression body — desugar to single Return
            let e = self.parse_expr()?;
            vec![Stmt::Return(Some(e))]
        };
        Ok(self.ast.add_expr(Expr::ArrowFn {
            params,
            return_type,
            body,
        }))
    }
}
