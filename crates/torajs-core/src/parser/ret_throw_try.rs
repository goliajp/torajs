//! Abrupt-completion statement cluster (chunk 419).
//!
//! Extracted verbatim from parser.rs — the statement parsers for
//! abrupt completions plus their shared block helper:
//! - parse_return — `return;` / `return expr;` with ASI
//! - parse_throw — `throw expr;`
//! - parse_try — `try {} catch (e) {} finally {}` incl. optional
//!   binding / destructured catch param
//! - parse_block_stmts — `{ ... }` as a flat stmt list (used by
//!   try / catch / finally bodies, not wrapped in Stmt::Block)
//!
//! parse_return / parse_throw / parse_try are called from
//! parse_stmt (parse_stmt.rs sibling); parse_block_stmts is
//! internal to this cluster. All promoted `pub(super)` per the
//! sibling-impl pack pattern. Body unchanged.

use super::destr_shape::PatShape;
use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_return(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `return`
        // ES §14.10 ReturnStatement is a restricted production:
        // `return [no LineTerminator here] Expression? ;` — a newline
        // between `return` and the next token forces `return;` even
        // if that token would otherwise start an Expression. Pre-fix
        // `function f() { return\n1; }` parsed as `return 1;`
        // (test262 language/asi/S7.9_A3.js: fn returns `undefined`,
        // tr returned 1).
        let expr = match self.peek() {
            Token::Semi | Token::RBrace | Token::Eof => None,
            _ if self.has_newline_before(self.pos) => None,
            _ => Some(self.parse_expr()?),
        };
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::Return(expr))
    }

    pub(super) fn parse_throw(&mut self) -> Result<Stmt, String> {
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
    pub(super) fn parse_try(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `try`
        let body = self.parse_block_stmts("try")?;
        // TS allows `try { } catch { }` OR `try { } finally { }` OR
        // `try { } catch { } finally { }` — at least one of catch /
        // finally is required.
        let mut catch_param: Option<String> = None;
        let mut catch_type: Option<String> = None;
        let mut catch_body: Vec<Stmt> = Vec::new();
        let mut had_catch = false;
        // §14.15 CatchParameter : BindingPattern — parsed by the shared
        // declaration-position PatShape machine (destr_shape.rs), so
        // defaults / nesting / rest / elision all work in a catch head
        // exactly as they do in `let PAT = src`. After the catch body
        // parses, emit_pattern_binds prepends the desugared lets
        // reading from the synthetic catch ident.
        let mut destr_pattern: Option<PatShape> = None;
        if matches!(self.peek(), Token::Catch) {
            had_catch = true;
            self.pos += 1;
            // `catch (e[: T])` is optional since ES2019.
            if matches!(self.peek(), Token::LParen) {
                self.pos += 1;
                let n = match self.peek() {
                    Token::Ident(n) => {
                        let s = n.clone();
                        self.pos += 1;
                        // §12.7.2 / §13.1.1 — the catch parameter is a
                        // BindingIdentifier, so strict code refuses
                        // `catch (eval)` the same way it refuses
                        // `var eval`. Judged where the name is read:
                        // unlike a function's parameter list, a catch
                        // clause has no directive prologue of its own,
                        // so the strictness in force is already final.
                        self.note_strict_binding(&s, self.in_strict_fn)?;
                        s
                    }
                    // §14.15 CatchParameter : BindingPattern
                    // (`catch ({ x = 1, ...r })`, `catch ([a, [b] = [],
                    // ...rest])`). The shared PatShape reader parses the
                    // pattern; the binds emit after the body parses. The
                    // synth catch param is typed `any` so the desugared
                    // reads route through the Any-tier.
                    Token::LBrace | Token::LBracket => {
                        let synth_name = format!("__catch_destr_{}", self.pos);
                        destr_pattern = Some(self.read_pattern_shape()?);
                        synth_name
                    }
                    t => {
                        return Err(format!(
                            "expected catch parameter name, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
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
                catch_param = Some(n.clone());
                // RC-3 — the catch parameter is an ordinary lexical
                // binding over the body that follows, so it drops any
                // class-value alias standing on the spelling for the
                // same reason a formal parameter does
                // (`finish_formal_params`).
                self.class_value_aliases.remove(&n);
                catch_type = ty;
                // Destructure forces `: any` catch type so the
                // synthesized reads route through the Any-tier.
                if destr_pattern.is_some() {
                    catch_type = Some("any".to_string());
                }
            }
            catch_body = self.parse_block_stmts("catch")?;
            // Desugar the catch pattern: emit_pattern_binds prepends
            // the bind chain reading from the synthetic catch ident.
            // The object lane's §13.3.3.5 RequireObjectCoercible guard
            // comes with the emitter (fires even for `catch ({})`).
            // Bindings are mutable — a catch parameter is an ordinary
            // lexical binding, assignable in the body.
            if let (Some(pat), Some(catch_n)) = (destr_pattern.take(), catch_param.clone()) {
                let src = self.ast.add_expr(Expr::Ident(catch_n));
                let mut prepend: Vec<Stmt> = Vec::new();
                self.emit_pattern_binds(&pat, src, true, false, &mut prepend);
                prepend.extend(catch_body.drain(..));
                catch_body = prepend;
            }
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
    pub(super) fn parse_block_stmts(&mut self, ctx: &str) -> Result<Vec<Stmt>, String> {
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
}
