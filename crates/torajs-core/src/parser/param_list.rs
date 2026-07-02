//! Parameter-list cluster (chunk 416).
//!
//! Extracted verbatim from parser.rs — the two shared param-list
//! parsers used by class methods / ctors and function-expression
//! forms:
//! - parse_ctor_param_list — ctor list with TS parameter-property
//!   shorthand (`constructor(public x: number, ...)`), returning the
//!   promoted-field side-table + destr-let prelude
//! - parse_param_list — plain `(p1: T, p2: T, ...)` list with
//!   destr-pattern / optional-`?` / default / rest wedges, returning
//!   the destr-let prelude for the caller to prepend
//!
//! Callers live in parse_class_member_method / object_member /
//! object_literal / fn_expr siblings. Both promoted `pub(super)`
//! per the sibling-impl pack pattern. Body unchanged.

use super::*;

impl<'a> Parser<'a> {
    /// V3-18 wedge — TS parameter-property shorthand
    /// (`constructor(public x: number, private readonly y: string)`).
    /// Returns the regular param list plus a side-table of
    /// (param_index, visibility, is_readonly) entries for params that
    /// should be promoted to instance fields, plus the destr-let vec
    /// for any binding-pattern params (synthesized `__param_destr_<id>`
    /// hidden bindings + per-element / per-field lets to prepend to the
    /// ctor body — caller does the prepend before promoted-prop assigns).
    /// Visibility / readonly modifiers can't combine with a destr
    /// pattern at the same param position (a binding pattern has no
    /// single field-name to promote).
    pub(super) fn parse_ctor_param_list(
        &mut self,
    ) -> Result<(Vec<Param>, Vec<(usize, ast::Visibility, bool)>, Vec<Stmt>), String> {
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => return Err(format!("expected `(`, got {t:?} at {}", self.at())),
        }
        let mut params = Vec::new();
        let mut promoted: Vec<(usize, ast::Visibility, bool)> = Vec::new();
        let mut destr_lets: Vec<Stmt> = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                // Consume any TS modifiers: visibility (`public` /
                // `private` / `protected`) and `readonly`. Order is
                // visibility-then-readonly per TS, but we accept any
                // combination once.
                let mut vis: Option<ast::Visibility> = None;
                let mut rd = false;
                loop {
                    let Token::Ident(s) = self.peek() else { break };
                    match s.as_str() {
                        "public" => {
                            if vis.is_some() {
                                return Err(format!(
                                    "duplicate visibility modifier in ctor param at {}",
                                    self.at()
                                ));
                            }
                            vis = Some(ast::Visibility::Public);
                            self.pos += 1;
                        }
                        "private" => {
                            if vis.is_some() {
                                return Err(format!(
                                    "duplicate visibility modifier in ctor param at {}",
                                    self.at()
                                ));
                            }
                            vis = Some(ast::Visibility::Private);
                            self.pos += 1;
                        }
                        "protected" => {
                            if vis.is_some() {
                                return Err(format!(
                                    "duplicate visibility modifier in ctor param at {}",
                                    self.at()
                                ));
                            }
                            vis = Some(ast::Visibility::Protected);
                            self.pos += 1;
                        }
                        "readonly" => {
                            if rd {
                                return Err(format!(
                                    "duplicate `readonly` in ctor param at {}",
                                    self.at()
                                ));
                            }
                            rd = true;
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
                let is_rest = matches!(self.peek(), Token::DotDotDot);
                if is_rest {
                    self.pos += 1;
                }
                if !is_rest && matches!(self.peek(), Token::LBracket | Token::LBrace) {
                    if vis.is_some() || rd {
                        return Err(format!(
                            "ctor destructuring param can't carry visibility / readonly modifiers at {}",
                            self.at()
                        ));
                    }
                    let synth = self.parse_destr_param(&mut destr_lets)?;
                    let type_ann = if matches!(self.peek(), Token::Colon) {
                        self.pos += 1;
                        Some(self.parse_type_ann()?)
                    } else {
                        None
                    };
                    params.push(Param {
                        name: synth,
                        type_ann,
                        default: None,
                        is_rest: false,
                    });
                    match self.peek() {
                        Token::Comma => {
                            self.pos += 1;
                            if matches!(self.peek(), Token::RParen) {
                                break;
                            }
                            continue;
                        }
                        Token::RParen => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `)` after destr ctor param, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
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
                let optional = !is_rest && matches!(self.peek(), Token::Question);
                if optional {
                    self.pos += 1;
                }
                let type_ann = if matches!(self.peek(), Token::Colon) {
                    self.pos += 1;
                    let ann = self.parse_type_ann()?;
                    if optional && !ann.starts_with("__nullable(") {
                        Some(format!("__nullable({ann})"))
                    } else {
                        Some(ann)
                    }
                } else {
                    None
                };
                let default = if !is_rest && matches!(self.peek(), Token::Eq) {
                    self.pos += 1;
                    Some(self.parse_expr()?)
                } else if optional {
                    // Implicit null default for `name?: T` (mirrors parse_fn).
                    Some(self.ast.add_expr(Expr::Null))
                } else {
                    None
                };
                let idx = params.len();
                params.push(Param {
                    name: pname,
                    type_ann,
                    default,
                    is_rest,
                });
                if vis.is_some() || rd {
                    promoted.push((idx, vis.unwrap_or(ast::Visibility::Public), rd));
                }
                match self.peek() {
                    Token::Comma => {
                        if is_rest {
                            return Err(format!("rest parameter must be last at {}", self.at()));
                        }
                        self.pos += 1;
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    }
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
        Ok((params, promoted, destr_lets))
    }

    /// Shared helper: parse a `(p1: T, p2: T, ...)` parameter list.
    /// Used by class methods/ctors. (Existing `parse_fn` / `parse_arrow_fn`
    /// have their own copies inlined; not refactoring them here to keep the
    /// M5.1 diff focused.)
    /// V3-18 wedge — return `(params, destr_lets)`. Destr_lets is the
    /// vec of `let bound = synth.field` (or `synth[i]`) statements
    /// generated when one or more params are binding patterns rather
    /// than identifiers. The caller is responsible for prepending
    /// destr_lets to the parsed body. When no destr params appear,
    /// destr_lets is empty and the caller's prepend is a no-op.
    pub(super) fn parse_param_list(&mut self) -> Result<(Vec<Param>, Vec<Stmt>), String> {
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => return Err(format!("expected `(`, got {t:?} at {}", self.at())),
        }
        let mut params = Vec::new();
        let mut param_destr_lets: Vec<Stmt> = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                // Rest parameter: `...name`. Must be the last param;
                // the post-loop check enforces it.
                let is_rest = matches!(self.peek(), Token::DotDotDot);
                if is_rest {
                    self.pos += 1;
                }
                if !is_rest && matches!(self.peek(), Token::LBracket | Token::LBrace) {
                    let synth = self.parse_destr_param(&mut param_destr_lets)?;
                    let type_ann = if matches!(self.peek(), Token::Colon) {
                        self.pos += 1;
                        Some(self.parse_type_ann()?)
                    } else {
                        None
                    };
                    // P-PARSE.6 — whole-pattern default on a destr
                    // method param: third call site for the same
                    // destr-default plumbing (parse_fn / parse_arrow_fn
                    // / class-method parse_param_list).
                    let default = if matches!(self.peek(), Token::Eq) {
                        self.pos += 1;
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    params.push(Param {
                        name: synth,
                        type_ann,
                        default,
                        is_rest: false,
                    });
                    match self.peek() {
                        Token::Comma => {
                            self.pos += 1;
                            if matches!(self.peek(), Token::RParen) {
                                break;
                            }
                            continue;
                        }
                        Token::RParen => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `)` after destr param, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
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
                // V3-18 wedge — optional parameter `name?: T`. Mirrors
                // the parse_fn version so class methods accept the
                // same shape.
                let optional = !is_rest && matches!(self.peek(), Token::Question);
                if optional {
                    self.pos += 1;
                }
                let type_ann = if matches!(self.peek(), Token::Colon) {
                    self.pos += 1;
                    let ann = self.parse_type_ann()?;
                    if optional && !ann.starts_with("__nullable(") {
                        Some(format!("__nullable({ann})"))
                    } else {
                        Some(ann)
                    }
                } else {
                    None
                };
                // Default value: `= <expr>`. Evaluated at the call
                // site (not in callee scope) when the caller omits
                // the arg. Not allowed on rest params. Optional `name?: T`
                // without an explicit default gets the implicit `null`
                // default (subset's undefined sentinel), matching parse_fn.
                let default = if !is_rest && matches!(self.peek(), Token::Eq) {
                    self.pos += 1;
                    Some(self.parse_expr()?)
                } else if optional {
                    Some(self.ast.add_expr(Expr::Null))
                } else {
                    None
                };
                params.push(Param {
                    name: pname,
                    type_ann,
                    default,
                    is_rest,
                });
                match self.peek() {
                    Token::Comma => {
                        if is_rest {
                            return Err(format!("rest parameter must be last at {}", self.at()));
                        }
                        self.pos += 1;
                        // V3-18 wedge — trailing comma in param list,
                        // per JS spec §13.3.3 ('function f(a, b,)'). Detect
                        // immediately-following ')' and break out.
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    }
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
        Ok((params, param_destr_lets))
    }
}
