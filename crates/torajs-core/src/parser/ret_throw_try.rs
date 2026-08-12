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
        // P4.7 — captured (binding-name, source-key, is_obj) tuples
        // when the catch param is a destructure pattern. After parsing
        // the catch body, we synthesize lets at the top of the body
        // that read each binding from the synthetic catch ident.
        let mut destr_bindings: Vec<(String, String, bool)> = Vec::new();
        // Some(is_obj) when the catch param was a binding pattern —
        // tracked independently of `destr_bindings` because the empty
        // object pattern (`catch ({})`) binds nothing yet still owes
        // the §13.3.3.5 RequireObjectCoercible throw on nullish.
        let mut destr_pattern_is_obj: Option<bool> = None;
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
                        self.note_strict_binding(&s)?;
                        s
                    }
                    // P4.7 — destructuring catch parameter
                    // (`catch ({ x, y }) {}`). ES2018+ BindingPattern
                    // syntax. tora parses the pattern at depth 1,
                    // captures the binding names, and (after the
                    // catch body parses) prepends synthetic
                    // `const x: any = __catch.x;` lets so the body's
                    // refs resolve. The synth catch param is typed
                    // `any` so the Member accesses route through
                    // dynobj_get via the Any-tier. Array destructure
                    // `catch ([a, b])` follows the same shape with
                    // string-keyed numeric indices.
                    Token::LBrace | Token::LBracket => {
                        let is_obj = matches!(self.peek(), Token::LBrace);
                        destr_pattern_is_obj = Some(is_obj);
                        let synth_name = format!("__catch_destr_{}", self.pos);
                        self.pos += 1; // skip opener
                        let mut idx = 0u32;
                        while !matches!(self.peek(), Token::RBrace | Token::RBracket | Token::Eof) {
                            let bind_name = match self.peek() {
                                Token::Ident(n) => {
                                    let s = n.clone();
                                    self.pos += 1;
                                    s
                                }
                                t => {
                                    return Err(format!(
                                        "expected identifier in catch destructure, got {t:?} at {}",
                                        self.at()
                                    ));
                                }
                            };
                            let stored_name = if is_obj && matches!(self.peek(), Token::Colon) {
                                self.pos += 1;
                                match self.peek() {
                                    Token::Ident(n) => {
                                        let s = n.clone();
                                        self.pos += 1;
                                        s
                                    }
                                    t => {
                                        return Err(format!(
                                            "expected alias after `:` in catch destructure, got {t:?} at {}",
                                            self.at()
                                        ));
                                    }
                                }
                            } else {
                                bind_name.clone()
                            };
                            let src_key = if is_obj {
                                bind_name
                            } else {
                                format!("{}", idx)
                            };
                            destr_bindings.push((stored_name, src_key, is_obj));
                            idx += 1;
                            if matches!(self.peek(), Token::Comma) {
                                self.pos += 1;
                            }
                        }
                        match self.peek() {
                            Token::RBrace | Token::RBracket => self.pos += 1,
                            _ => {}
                        }
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
                catch_type = ty;
                // P4.7 — destructure forces `: any` catch type so the
                // synthesized Member-reads route through the Any-tier.
                if destr_pattern_is_obj.is_some() {
                    catch_type = Some("any".to_string());
                }
            }
            catch_body = self.parse_block_stmts("catch")?;
            // P4.7 — synthesize `const <bind>: any = <__catch>.<src_key>;`
            // for each destructure binding and prepend to catch_body.
            // An object pattern additionally owes the §13.3.3.5
            // RequireObjectCoercible guard even when it binds nothing.
            if let (Some(is_obj), Some(catch_n)) =
                (destr_pattern_is_obj, catch_param.as_ref().cloned())
            {
                let mut prepend: Vec<Stmt> = Vec::with_capacity(destr_bindings.len() + 1);
                if is_obj {
                    prepend.push(self.emit_object_coercible_guard(&catch_n));
                }
                for (bind_name, src_key, _is_obj) in destr_bindings.iter() {
                    let obj_eid = self.ast.add_expr(Expr::Ident(catch_n.clone()));
                    let member_eid = self.ast.add_expr(Expr::Member {
                        obj: obj_eid,
                        name: src_key.clone(),
                    });
                    prepend.push(Stmt::LetDecl {
                        mutable: false,
                        name: bind_name.clone(),
                        type_ann: Some("any".to_string()),
                        init: member_eid,
                        is_var: false,
                    });
                }
                let mut new_body: Vec<Stmt> = Vec::with_capacity(catch_body.len() + prepend.len());
                new_body.extend(prepend);
                new_body.extend(catch_body.drain(..));
                catch_body = new_body;
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
