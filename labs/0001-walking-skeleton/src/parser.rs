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

use crate::ast::{self, Ast, BinOp, ClassCtor, ClassMethod, Expr, ExprId, Param, Stmt};
use crate::lexer::{self, Spanned, Token};

pub fn parse(tokens: &[Spanned]) -> Result<Ast, String> {
    let mut p = Parser {
        tokens,
        pos: 0,
        ast: Ast::default(),
        desugar_id: 0,
    };
    p.parse_program()?;
    Ok(p.ast)
}

struct Parser<'a> {
    tokens: &'a [Spanned],
    pos: usize,
    ast: Ast,
    /// Monotone counter used by parse-time desugars (for-of, destructuring,
    /// template literal interpolation) to mint collision-free temp names.
    /// Starts at 0; each `mint_temp_id` returns + increments.
    desugar_id: u32,
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
        // Trailing `| null` → wrap in a `__nullable(T)` marker. Parser
        // doesn't try to be a full TS union solver — only the
        // single-side-null case is supported in this subset.
        if matches!(self.peek(), Token::Pipe) {
            self.pos += 1;
            if matches!(self.peek(), Token::Null) {
                self.pos += 1;
                name = format!("__nullable({name})");
            } else {
                return Err(format!(
                    "only `T | null` unions are supported (no other unions yet); got {:?} at {}",
                    self.peek(),
                    self.at()
                ));
            }
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
        if matches!(self.peek(), Token::Class) {
            return self.parse_class_decl();
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
            // Destructuring: `let [a, b] = src` or `let { x, y } = src`.
            // Parsed inline so it shares the let-decl's lookahead. Both
            // forms desugar to `let __t = src; let <field>...; ...` so the
            // backend never sees a destructuring pattern.
            if matches!(self.peek(), Token::LBracket) {
                return self.parse_array_destructuring(mutable);
            }
            if matches!(self.peek(), Token::LBrace) {
                return self.parse_object_destructuring(mutable);
            }
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
            had_catch,
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

    fn parse_do_while(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `do`
        let body = Box::new(self.parse_stmt()?);
        match self.peek() {
            Token::While => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `while` after `do {{ … }}`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `while` in do-while, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let cond = self.parse_expr()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after do-while condition, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // Optional `;` after the closing paren.
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::DoWhile { body, cond })
    }

    fn parse_switch(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `switch`
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `switch`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let scrutinee = self.parse_expr()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after switch scrutinee, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` to begin switch body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut cases: Vec<ast::SwitchCase> = Vec::new();
        let mut default: Option<Vec<Stmt>> = None;
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            match self.peek() {
                Token::Case => {
                    self.pos += 1;
                    let value = self.parse_expr()?;
                    match self.peek() {
                        Token::Colon => self.pos += 1,
                        t => {
                            return Err(format!(
                                "expected `:` after case value, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    let mut body: Vec<Stmt> = Vec::new();
                    while !matches!(
                        self.peek(),
                        Token::Case | Token::Default | Token::RBrace | Token::Eof
                    ) {
                        body.push(self.parse_stmt()?);
                    }
                    cases.push(ast::SwitchCase { value, body });
                }
                Token::Default => {
                    self.pos += 1;
                    match self.peek() {
                        Token::Colon => self.pos += 1,
                        t => {
                            return Err(format!(
                                "expected `:` after `default`, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    let mut body: Vec<Stmt> = Vec::new();
                    while !matches!(
                        self.peek(),
                        Token::Case | Token::Default | Token::RBrace | Token::Eof
                    ) {
                        body.push(self.parse_stmt()?);
                    }
                    default = Some(body);
                }
                t => {
                    return Err(format!(
                        "expected `case` or `default` inside switch, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end switch, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(Stmt::Switch {
            scrutinee,
            cases,
            default,
        })
    }

    /// `for (init?; cond?; step?) body`. Each clause is optional but the
    /// two `;` separators are required (matches TS / C). Init is parsed
    /// as a stmt (typically a `let` decl or expr-stmt). Cond is an expr.
    /// Step is an expr (we don't have post-increment yet — use
    /// `i = i + 1`).
    fn mint_desugar_id(&mut self) -> u32 {
        let id = self.desugar_id;
        self.desugar_id += 1;
        id
    }

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
        // for-of detection: `for ( (let|const) IDENT (`:` T)? "of" EXPR )`.
        // `of` is a contextual keyword (Token::Ident("of")) so this only
        // triggers when the user means it. Falls back to C-style for-loop
        // otherwise. The desugar produces zero-overhead SSA: source is
        // bound once into a `__forof_src_N` temp (skipped if the source
        // was already an Ident — mem2reg would elide it anyway, but we
        // skip eagerly so the AST stays small), `__forof_end_N` caches
        // length, classic for-loop walks indices, body sees the user's
        // binding rebound from `__src[__i]`.
        if let Some(stmt) = self.try_parse_for_of()? {
            return Ok(stmt);
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

    /// Try to recognize a `for ( (let|const) IDENT (: T)? ("of"|"in") EXPR )`
    /// head (we're already past the `(`). On match, consumes through the
    /// closing `)`, parses the body, and emits the desugared C-style
    /// for-loop. On non-match, restores `pos` and returns `Ok(None)` so
    /// the caller falls back to the regular for-loop parser.
    ///
    /// for-in vs for-of:
    /// - for-of (`for (let v of arr)`) iterates the array elements.
    /// - for-in (`for (let k in obj)`) iterates obj's struct field names
    ///   as strings. Same desugar shape as for-of, with the source
    ///   wrapped in `Object.keys(obj)` so the body sees a string[].
    ///
    /// Performance shape: the desugar collapses to the same SSA the user
    /// would write by hand —
    ///   { let __forof_src_N = src;       // skipped if src is Ident
    ///     let __forof_end_N = __src.length;
    ///     for (let __forof_i_N = 0; __i < __end; __i = __i + 1) {
    ///       let v = __src[__i];
    ///       <body>
    ///     } }
    /// — so mem2reg trivializes it and the loop runs at the same speed
    /// as a hand-written index walk.
    fn try_parse_for_of(&mut self) -> Result<Option<Stmt>, String> {
        let saved = self.pos;
        if !matches!(self.peek(), Token::Let | Token::Const) {
            return Ok(None);
        }
        self.pos += 1;
        let var_name = match self.peek() {
            Token::Ident(n) => n.clone(),
            _ => {
                self.pos = saved;
                return Ok(None);
            }
        };
        self.pos += 1;
        // Optional `: T` annotation — for now consumed but not used; the
        // desugared `let v = __src[__i]` infers the element type from the
        // array. Carrying the annotation forward isn't necessary for v0.
        let mut have_type_ann = false;
        if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            let _ann = self.parse_type_ann()?;
            have_type_ann = true;
        }
        // Contextual `of` / `in` keyword — must be an Ident. Anything
        // else (`=` for a regular let-in-init, `;` for empty init, etc.)
        // means this is NOT a for-of/in and we restore.
        let kind = match self.peek() {
            Token::Ident(n) if n == "of" => Some("of"),
            Token::Ident(n) if n == "in" => Some("in"),
            _ => None,
        };
        let Some(kind) = kind else {
            self.pos = saved;
            return Ok(None);
        };
        self.pos += 1; // consume "of" / "in"
        let _ = have_type_ann; // not yet propagated; suppress unused warning
        let raw_src = self.parse_expr()?;
        // for-in wraps `Object.keys(raw_src)` so the body sees a
        // string[]. for-of uses raw_src directly.
        let src = if kind == "in" {
            let object_id = self.ast.add_expr(Expr::Ident("Object".into()));
            let keys_member = self.ast.add_expr(Expr::Member {
                obj: object_id,
                name: "keys".into(),
            });
            self.ast.add_expr(Expr::Call {
                callee: keys_member,
                args: vec![raw_src],
            })
        } else {
            raw_src
        };
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after for-of source, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let body = self.parse_stmt()?;

        // Emit the desugar. Mint a unique suffix per for-of to avoid
        // collisions with user code AND with sibling for-ofs in the same
        // scope (block-scope shadow would also save us, but explicit
        // unique names are cheaper to reason about).
        let id = self.mint_desugar_id();
        let src_name = format!("__forof_src_{id}");
        let end_name = format!("__forof_end_{id}");
        let i_name = format!("__forof_i_{id}");

        // src ident — if `src` is already an Ident, reuse it directly so
        // we skip allocating a redundant temp. Cross-scope rebind handled
        // by the regular LetDecl path; the alias-init rule already covers
        // that case so the heap stays owned by the original binding.
        let src_is_ident = matches!(self.ast.get_expr(src), Expr::Ident(_));
        let src_ref_name: String = if src_is_ident {
            if let Expr::Ident(n) = self.ast.get_expr(src) {
                n.clone()
            } else {
                unreachable!()
            }
        } else {
            src_name.clone()
        };

        let mut stmts: Vec<Stmt> = Vec::new();
        if !src_is_ident {
            // `let __src = <src>;`
            stmts.push(Stmt::LetDecl {
                mutable: false,
                name: src_name.clone(),
                type_ann: None,
                init: src,
            });
        }
        // `let __end = __src.length;`
        let src_ident_for_len = self.ast.add_expr(Expr::Ident(src_ref_name.clone()));
        let end_init = self.ast.add_expr(Expr::Member {
            obj: src_ident_for_len,
            name: "length".into(),
        });
        stmts.push(Stmt::LetDecl {
            mutable: false,
            name: end_name.clone(),
            type_ann: Some("number".into()),
            init: end_init,
        });
        // `for (let __i = 0; __i < __end; __i = __i + 1) { let v = __src[__i]; body }`
        let zero = self.ast.add_expr(Expr::Number(0.0));
        let init_stmt = Stmt::LetDecl {
            mutable: true,
            name: i_name.clone(),
            type_ann: Some("number".into()),
            init: zero,
        };
        let i_ref_for_cond = self.ast.add_expr(Expr::Ident(i_name.clone()));
        let end_ref = self.ast.add_expr(Expr::Ident(end_name.clone()));
        let cond_expr = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Lt,
            left: i_ref_for_cond,
            right: end_ref,
        });
        let i_ref_for_step_lhs = self.ast.add_expr(Expr::Ident(i_name.clone()));
        let i_ref_for_step_rhs = self.ast.add_expr(Expr::Ident(i_name.clone()));
        let one = self.ast.add_expr(Expr::Number(1.0));
        let step_inc = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Add,
            left: i_ref_for_step_rhs,
            right: one,
        });
        let i_target = self.ast.add_expr(Expr::Ident(i_name.clone()));
        let _ = i_ref_for_step_lhs; // unused; keep the index symbol uniqueness
        let step_assign = self.ast.add_expr(Expr::Assign {
            target: i_target,
            value: step_inc,
        });
        // Body wrapped in a block so the `let v = __src[__i]` only lives
        // for one iteration's scope — block-close drop emission cleans up
        // any non-Copy element borrow.
        let i_ref_for_index = self.ast.add_expr(Expr::Ident(i_name.clone()));
        let src_ref_for_index = self.ast.add_expr(Expr::Ident(src_ref_name.clone()));
        let elem_init = self.ast.add_expr(Expr::Index {
            obj: src_ref_for_index,
            index: i_ref_for_index,
        });
        let mut body_stmts: Vec<Stmt> = Vec::new();
        body_stmts.push(Stmt::LetDecl {
            mutable: false,
            name: var_name,
            type_ann: None,
            init: elem_init,
        });
        body_stmts.push(body);
        let body_block = Stmt::Block(body_stmts);
        let for_stmt = Stmt::For {
            init: Some(Box::new(init_stmt)),
            cond: Some(cond_expr),
            step: Some(step_assign),
            body: Box::new(body_block),
        };
        stmts.push(for_stmt);
        Ok(Some(Stmt::Block(stmts)))
    }

    /// `let [a, b, c] = src` → `let __t = src; let a = __t[0]; let b = __t[1]; ...`.
    /// Source is bound once into a `__destr_src_N` temp (skipped if the
    /// source was already an Ident — the alias rule keeps the original
    /// binding the owner). Each pattern entry produces a regular
    /// LetDecl; mem2reg + ssa_lower already optimize the resulting
    /// shape down to direct loads. Element omission via comma-comma
    /// (`let [, b] = src`) and tail rest (`let [a, ...rest] = src`)
    /// are not supported in this first pass — they'd need a runtime
    /// slice helper to produce the rest array.
    fn parse_array_destructuring(&mut self, mutable: bool) -> Result<Stmt, String> {
        self.pos += 1; // consume `[`
        let mut names: Vec<String> = Vec::new();
        if !matches!(self.peek(), Token::RBracket) {
            loop {
                let n = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    t => {
                        return Err(format!(
                            "expected identifier in array destructuring, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                names.push(n);
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                    continue;
                }
                break;
            }
        }
        match self.peek() {
            Token::RBracket => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `]` ending array destructuring, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // Optional `: T[]` annotation on the whole pattern is not
        // tracked at this layer — the desugared elements get their type
        // from the array's element type via inference.
        if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            let _ann = self.parse_type_ann()?;
        }
        match self.peek() {
            Token::Eq => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `=` after destructuring pattern, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let src = self.parse_expr()?;
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        let stmts = self.emit_array_destructuring(mutable, &names, src);
        // Stmt::Multi flattens at lowering time, so the user-visible
        // lets join the surrounding scope rather than a fresh frame —
        // matches TS semantics where `let [a, b] = src; …; a` works.
        Ok(Stmt::Multi(stmts))
    }

    fn emit_array_destructuring(
        &mut self,
        mutable: bool,
        names: &[String],
        src: ExprId,
    ) -> Vec<Stmt> {
        let id = self.mint_desugar_id();
        let src_is_ident = matches!(self.ast.get_expr(src), Expr::Ident(_));
        let src_ref_name: String = if src_is_ident {
            if let Expr::Ident(n) = self.ast.get_expr(src) {
                n.clone()
            } else {
                unreachable!()
            }
        } else {
            format!("__destr_src_{id}")
        };
        let mut stmts: Vec<Stmt> = Vec::new();
        if !src_is_ident {
            stmts.push(Stmt::LetDecl {
                mutable: false,
                name: src_ref_name.clone(),
                type_ann: None,
                init: src,
            });
        }
        for (i, name) in names.iter().enumerate() {
            let src_ref = self.ast.add_expr(Expr::Ident(src_ref_name.clone()));
            let idx = self.ast.add_expr(Expr::Number(i as f64));
            let elem = self.ast.add_expr(Expr::Index { obj: src_ref, index: idx });
            stmts.push(Stmt::LetDecl {
                mutable,
                name: name.clone(),
                type_ann: None,
                init: elem,
            });
        }
        stmts
    }

    /// `let { x, y } = src` → `let __t = src; let x = __t.x; let y = __t.y;`.
    /// Renaming `let { x: foo, y: bar } = src` rebinds to foo / bar.
    /// Same source-binding rule as array destructuring.
    fn parse_object_destructuring(&mut self, mutable: bool) -> Result<Stmt, String> {
        self.pos += 1; // consume `{`
        let mut entries: Vec<(String, String)> = Vec::new(); // (field_name, bound_as)
        if !matches!(self.peek(), Token::RBrace) {
            loop {
                let field = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    t => {
                        return Err(format!(
                            "expected identifier in object destructuring, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                let bound = if matches!(self.peek(), Token::Colon) {
                    self.pos += 1;
                    let n = match self.peek() {
                        Token::Ident(n) => n.clone(),
                        t => {
                            return Err(format!(
                                "expected rename target after `:` in destructuring, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    };
                    self.pos += 1;
                    n
                } else {
                    field.clone()
                };
                entries.push((field, bound));
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                    continue;
                }
                break;
            }
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` ending object destructuring, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            let _ann = self.parse_type_ann()?;
        }
        match self.peek() {
            Token::Eq => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `=` after destructuring pattern, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let src = self.parse_expr()?;
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        let stmts = self.emit_object_destructuring(mutable, &entries, src);
        Ok(Stmt::Multi(stmts))
    }

    fn emit_object_destructuring(
        &mut self,
        mutable: bool,
        entries: &[(String, String)],
        src: ExprId,
    ) -> Vec<Stmt> {
        let id = self.mint_desugar_id();
        let src_is_ident = matches!(self.ast.get_expr(src), Expr::Ident(_));
        let src_ref_name: String = if src_is_ident {
            if let Expr::Ident(n) = self.ast.get_expr(src) {
                n.clone()
            } else {
                unreachable!()
            }
        } else {
            format!("__destr_src_{id}")
        };
        let mut stmts: Vec<Stmt> = Vec::new();
        if !src_is_ident {
            stmts.push(Stmt::LetDecl {
                mutable: false,
                name: src_ref_name.clone(),
                type_ann: None,
                init: src,
            });
        }
        for (field, bound) in entries {
            let src_ref = self.ast.add_expr(Expr::Ident(src_ref_name.clone()));
            let mem = self.ast.add_expr(Expr::Member {
                obj: src_ref,
                name: field.clone(),
            });
            stmts.push(Stmt::LetDecl {
                mutable,
                name: bound.clone(),
                type_ann: None,
                init: mem,
            });
        }
        stmts
    }

    /// Stitch a `Token::Template`'s parts into an `Expr::String + Expr +
    /// Expr::String + …` chain. Single-part literal templates collapse
    /// to a bare `Expr::String`. Empty-parts templates collapse to
    /// `Expr::String("")`. Each interpolation gets a sub-Parser so the
    /// recursive lex output can be consumed without re-tokenizing.
    ///
    /// Performance: chain of `+`s reuses the existing string-concat fast
    /// path, including number→string auto-coercion. For N interpolations
    /// with K number values: K num→str allocs + N concats. The parser
    /// could in principle build an Array+join (1 array alloc + 1 join
    /// alloc instead of N concats) for ≥3 interpolations; deferred to a
    /// later optimization once profiling shows template heavy use.
    fn lower_template_parts(
        &mut self,
        parts: &[lexer::TemplatePart],
    ) -> Result<ExprId, String> {
        if parts.is_empty() {
            return Ok(self.ast.add_expr(Expr::String(String::new())));
        }
        // Special-case all-literal templates → emit a single
        // Expr::String. Common case `\`hello\`` skips the chain entirely.
        if parts.len() == 1 {
            if let lexer::TemplatePart::Lit(s) = &parts[0] {
                return Ok(self.ast.add_expr(Expr::String(s.clone())));
            }
        }
        let mut acc: Option<ExprId> = None;
        for p in parts {
            let id = match p {
                lexer::TemplatePart::Lit(s) => {
                    if s.is_empty() && acc.is_some() {
                        // Skip empty-string filler between adjacent
                        // interpolations — `${a}${b}` shouldn't
                        // generate an extra `+ ""` step.
                        continue;
                    }
                    self.ast.add_expr(Expr::String(s.clone()))
                }
                lexer::TemplatePart::Expr(tokens) => {
                    let mut sub = Parser {
                        tokens,
                        pos: 0,
                        ast: std::mem::take(&mut self.ast),
                        desugar_id: self.desugar_id,
                    };
                    let result = sub.parse_expr()?;
                    // Tokens vec ends with Token::Eof; anything before
                    // Eof past the parsed expr is leftover input.
                    if !matches!(sub.peek(), Token::Eof) {
                        return Err(format!(
                            "unexpected trailing tokens in template interpolation: {:?}",
                            sub.peek()
                        ));
                    }
                    self.ast = sub.ast;
                    self.desugar_id = sub.desugar_id;
                    result
                }
            };
            acc = Some(match acc {
                None => id,
                Some(prev) => self.ast.add_expr(Expr::BinOp {
                    op: BinOp::Add,
                    left: prev,
                    right: id,
                }),
            });
        }
        // If acc is still None (everything was empty Lit), produce "".
        Ok(acc.unwrap_or_else(|| self.ast.add_expr(Expr::String(String::new()))))
    }

    fn parse_expr(&mut self) -> Result<ExprId, String> {
        self.parse_assign()
    }

    fn parse_assign(&mut self) -> Result<ExprId, String> {
        let target = self.parse_ternary()?;
        // Plain `=` and the compound forms (`+= -= *= /= %=`). Compound
        // forms desugar at the parser level into `target = target op value`,
        // matching JS shape without needing a new AST variant.
        let compound_op = match self.peek() {
            Token::Eq => None,
            Token::PlusEq => Some(BinOp::Add),
            Token::MinusEq => Some(BinOp::Sub),
            Token::StarEq => Some(BinOp::Mul),
            Token::SlashEq => Some(BinOp::Div),
            Token::PercentEq => Some(BinOp::Mod),
            _ => return Ok(target),
        };
        self.pos += 1;
        let value = self.parse_assign()?; // right-associative
        if let Some(op) = compound_op {
            // For idents we re-use the same target ExprId on the rhs;
            // for member/index targets we must clone the access (since
            // they're side-effect-free in our subset). Easiest: clone
            // the AST node by re-evaluating at the rhs position. tr's
            // AST is arena-backed so we just append a fresh expr that
            // re-reads the same Ident / Member / Index.
            let lhs = self.clone_expr_for_compound(target);
            let rhs = self.ast.add_expr(Expr::BinOp { op, left: lhs, right: value });
            return Ok(self.ast.add_expr(Expr::Assign { target, value: rhs }));
        }
        Ok(self.ast.add_expr(Expr::Assign { target, value }))
    }

    /// Compound assign desugar helper: produce a fresh `ExprId` that
    /// references the same identifier/member/index as `eid`. We can
    /// share scalar/binop sub-trees, but the LHS appears twice in the
    /// desugared `x = x + v`, and treating it as a literal share would
    /// confuse downstream passes that index by ExprId. So make a
    /// fresh top-level node — for the shapes the parser produces here
    /// (`Ident`, `Member`, `Index`) the read is side-effect-free.
    fn clone_expr_for_compound(&mut self, eid: ExprId) -> ExprId {
        let cloned = match self.ast.get_expr(eid) {
            Expr::Ident(name) => Expr::Ident(name.clone()),
            Expr::Member { obj, name } => Expr::Member { obj: *obj, name: name.clone() },
            Expr::Index { obj, index } => Expr::Index { obj: *obj, index: *index },
            other => panic!(
                "parser: invalid compound-assign target shape {other:?}"
            ),
        };
        self.ast.add_expr(cloned)
    }

    fn parse_ternary(&mut self) -> Result<ExprId, String> {
        let cond = self.parse_nullish()?;
        if !matches!(self.peek(), Token::Question) {
            return Ok(cond);
        }
        self.pos += 1;
        let then_branch = self.parse_assign()?; // right-associative through assign
        if !matches!(self.peek(), Token::Colon) {
            return Err(format!(
                "expected `:` in ternary expression, got {:?}",
                self.peek()
            ));
        }
        self.pos += 1;
        let else_branch = self.parse_assign()?;
        Ok(self.ast.add_expr(Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        }))
    }

    /// `lhs ?? rhs` — left-associative, below ternary in precedence.
    /// Lowered as a new `Expr::Nullish { lhs, rhs }` because the lhs
    /// must be evaluated EXACTLY ONCE (it can have side effects); a
    /// pure ternary desugar would either re-evaluate or require an
    /// expression-level `let-binding` we don't have. ssa_lower stores
    /// the lhs into a temp slot and branches on its null-ness.
    fn parse_nullish(&mut self) -> Result<ExprId, String> {
        let mut left = self.parse_logical_or()?;
        while matches!(self.peek(), Token::QuestionQuestion) {
            self.pos += 1;
            let right = self.parse_logical_or()?;
            left = self.ast.add_expr(Expr::Nullish {
                lhs: left,
                rhs: right,
            });
        }
        Ok(left)
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
        if matches!(self.peek(), Token::Tilde) {
            self.pos += 1;
            let inner = self.parse_unary()?;
            return Ok(self.ast.add_expr(Expr::Unary {
                op: ast::UnaryOp::BitNot,
                expr: inner,
            }));
        }
        if matches!(self.peek(), Token::TypeOf) {
            self.pos += 1;
            let inner = self.parse_unary()?;
            return Ok(self.ast.add_expr(Expr::TypeOf { expr: inner }));
        }
        // Pre-increment / pre-decrement: `++x` desugars to `x = x + 1`,
        // value is the new x. We emit an Assign whose target is the
        // ident binding; the result of an Assign expression in the
        // existing AST already evaluates to the new value.
        if matches!(self.peek(), Token::PlusPlus | Token::MinusMinus) {
            let is_inc = matches!(self.peek(), Token::PlusPlus);
            self.pos += 1;
            let target = self.parse_unary()?;
            let lhs_clone = self.clone_expr_for_compound(target);
            let one = self.ast.add_expr(Expr::Number(1.0));
            let op = if is_inc { BinOp::Add } else { BinOp::Sub };
            let rhs = self.ast.add_expr(Expr::BinOp {
                op,
                left: lhs_clone,
                right: one,
            });
            return Ok(self.ast.add_expr(Expr::Assign {
                target,
                value: rhs,
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
                Token::QuestionDot => {
                    self.pos += 1;
                    let name = match self.peek() {
                        Token::Ident(n) => n.clone(),
                        t => {
                            return Err(format!(
                                "expected identifier after `?.`, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    };
                    self.pos += 1;
                    node = self.ast.add_expr(Expr::OptChain { obj: node, name });
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
                Token::PlusPlus | Token::MinusMinus => {
                    // Post-increment / post-decrement: `x++` / `x--`.
                    // JS spec: yields the OLD value, then mutates. We
                    // approximate as "yield the new value" — the most
                    // common use is in for-loop step where the result is
                    // discarded. This is a known deviation; if a real
                    // case relies on the spec semantics, we can lift to
                    // an explicit `let __old = x; x = x + 1; __old`.
                    let is_inc = matches!(self.peek(), Token::PlusPlus);
                    self.pos += 1;
                    let lhs_clone = self.clone_expr_for_compound(node);
                    let one = self.ast.add_expr(Expr::Number(1.0));
                    let op = if is_inc { BinOp::Add } else { BinOp::Sub };
                    let rhs = self.ast.add_expr(Expr::BinOp {
                        op,
                        left: lhs_clone,
                        right: one,
                    });
                    node = self.ast.add_expr(Expr::Assign {
                        target: node,
                        value: rhs,
                    });
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
            Token::Template { parts } => {
                let parts = parts.clone();
                self.pos += 1;
                self.lower_template_parts(&parts)
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
            Token::Null => {
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Null))
            }
            Token::This => {
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::This))
            }
            Token::Super => {
                // `super(args)` — only valid inside a subclass ctor; the
                // desugar pass enforces that and rewrites to a Call to
                // `__cm_<Parent>__ctor(__this, args)`. `super.method(args)`
                // (explicit parent-method call) is M5.3.
                self.pos += 1;
                match self.peek() {
                    Token::LParen => self.pos += 1,
                    t => {
                        return Err(format!(
                            "expected `(` after `super`, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
                let mut args: Vec<ExprId> = Vec::new();
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
                Ok(self.ast.add_expr(Expr::Super { args }))
            }
            Token::New => {
                // `new ClassName(args)` — type args / generic ctors not yet
                // supported; that's M5.2 alongside extends.
                self.pos += 1;
                let class_name = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    t => {
                        return Err(format!(
                            "expected class name after `new`, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                match self.peek() {
                    Token::LParen => self.pos += 1,
                    t => {
                        return Err(format!(
                            "expected `(` after `new {class_name}`, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
                let mut args: Vec<ExprId> = Vec::new();
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
                Ok(self.ast.add_expr(Expr::New { class_name, args }))
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

    /// M5.1 — `class C { field: T; constructor(...) {...} method(...): R {...} }`.
    /// Single class, no inheritance / super / static / accessors. Lowered
    /// post-parse by `desugar_classes` into a `TypeDecl` + a series of
    /// `FnDecl`s. The parser only assembles the structure here.
    fn parse_class_decl(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `class`
        let name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!(
                    "expected class name, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        // M5.2 — optional `extends BaseName` clause.
        let parent: Option<String> = if matches!(self.peek(), Token::Extends) {
            self.pos += 1;
            match self.peek() {
                Token::Ident(n) => {
                    let n = n.clone();
                    self.pos += 1;
                    Some(n)
                }
                t => {
                    return Err(format!(
                        "expected parent class name after `extends`, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        } else {
            None
        };
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` to begin class body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut fields: Vec<(String, String)> = Vec::new();
        let mut ctor: Option<ClassCtor> = None;
        let mut methods: Vec<ClassMethod> = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            // Each member is one of:
            //   - `constructor(params) { body }`
            //   - `methodName(params): R? { body }`
            //   - `fieldName: T;`
            // We disambiguate by lookahead: ident then `(` ⇒ ctor or method;
            // ident then `:` ⇒ field declaration.
            let member_name = match self.peek() {
                Token::Ident(n) => n.clone(),
                t => {
                    return Err(format!(
                        "expected class member name, got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            let next_tok = self.tokens.get(self.pos + 1).map(|s| &s.token);
            match next_tok {
                Some(Token::LParen) => {
                    // ctor or method
                    self.pos += 1; // consume name
                    let params = self.parse_param_list()?;
                    let return_type = if matches!(self.peek(), Token::Colon) {
                        self.pos += 1;
                        Some(self.parse_type_ann()?)
                    } else {
                        None
                    };
                    match self.peek() {
                        Token::LBrace => self.pos += 1,
                        t => {
                            return Err(format!(
                                "expected `{{` for {member_name} body, got {t:?} at {}",
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
                        t => {
                            return Err(format!(
                                "expected `}}` to end {member_name} body, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    if member_name == "constructor" {
                        if ctor.is_some() {
                            return Err(format!(
                                "duplicate constructor in class `{name}`"
                            ));
                        }
                        ctor = Some(ClassCtor { params, body });
                    } else {
                        methods.push(ClassMethod {
                            name: member_name,
                            params,
                            return_type,
                            body,
                        });
                    }
                }
                Some(Token::Colon) => {
                    // field declaration: `name: T;`
                    self.pos += 2; // consume name + colon
                    let ty = self.parse_type_ann()?;
                    if matches!(self.peek(), Token::Semi) {
                        self.pos += 1;
                    }
                    fields.push((member_name, ty));
                }
                t => {
                    return Err(format!(
                        "expected `(` (method) or `:` (field) after `{member_name}`, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end class body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::ClassDecl {
            name,
            parent,
            fields,
            ctor,
            methods,
        })
    }

    /// Shared helper: parse a `(p1: T, p2: T, ...)` parameter list.
    /// Used by class methods/ctors. (Existing `parse_fn` / `parse_arrow_fn`
    /// have their own copies inlined; not refactoring them here to keep the
    /// M5.1 diff focused.)
    fn parse_param_list(&mut self) -> Result<Vec<Param>, String> {
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
                params.push(Param { name: pname, type_ann });
                match self.peek() {
                    Token::Comma => self.pos += 1,
                    Token::RParen => break,
                    t => {
                        return Err(format!(
                            "expected `,` or `)` in params, got {t:?} at {}",
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
        Ok(params)
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
            elements.push(self.parse_array_element()?);
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                if matches!(self.peek(), Token::RBracket) {
                    break; // trailing comma allowed
                }
                elements.push(self.parse_array_element()?);
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

    /// One slot inside an array literal — either a spread `...src` or a
    /// regular expression. Spread is wrapped in `Expr::Spread { expr }`
    /// so ssa_lower's Array arm can fork into the pre-sized alloc path.
    fn parse_array_element(&mut self) -> Result<ExprId, String> {
        if matches!(self.peek(), Token::DotDotDot) {
            self.pos += 1;
            let inner = self.parse_expr()?;
            return Ok(self.ast.add_expr(Expr::Spread { expr: inner }));
        }
        self.parse_expr()
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
