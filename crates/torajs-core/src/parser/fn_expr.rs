//! Function-expression cluster (chunk 414).
//!
//! Extracted verbatim from parser.rs — the three methods behind
//! function values in expression position:
//! - is_arrow_fn_at_lparen — lookahead from `(` for `=>` (incl.
//!   `(): T => ...` return-ann forms) to disambiguate arrow-fn vs
//!   parenthesized expression
//! - parse_fn_expr — `function (params): R { body }` /
//!   `function NAME(...)` / `function*() {}` stub-drop, all emitted
//!   as Expr::ArrowFn
//! - parse_arrow_fn — `(params) => body` with destr-param /
//!   optional-param / trailing-comma wedges and expression-body →
//!   single-Return desugar
//!
//! All three are called from parse_primary (parser.rs); promoted
//! `pub(super)` per the sibling-impl pack pattern. Body unchanged.

use super::*;

impl<'a> Parser<'a> {
    /// Lookahead: from a `(` at `self.pos`, find the matching `)` and peek
    /// for `=>` (or `: T => ...`) to decide arrow-fn vs parenthesized expression.
    /// Handles nested parens correctly.
    pub(super) fn is_arrow_fn_at_lparen(&self) -> bool {
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
                        if matches!(self.tokens.get(i + 1).map(|s| &s.token), Some(Token::Colon)) {
                            // Scan past the type ann to look for `=>`.
                            let mut j = i + 2;
                            // Type starts with an identifier — or
                            // the `void` keyword (m1.h.30 promoted
                            // it from contextual ident to keyword;
                            // arrow lookahead must accept it here
                            // for `(): void => ...` to parse).
                            if !matches!(
                                self.tokens.get(j).map(|s| &s.token),
                                Some(Token::Ident(_)) | Some(Token::Void)
                            ) {
                                return false;
                            }
                            j += 1;
                            // Optional generic args `<T1, T2, ...>` —
                            // very rough scan; if we hit a stray `>`
                            // before the next plausible `=>`, fall back
                            // to "not arrow".
                            if matches!(self.tokens.get(j).map(|s| &s.token), Some(Token::Lt)) {
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
                            // V3-18 wedge — trailing `| null` (the only
                            // union shape this subset's type-ann
                            // parser supports). Allow `() : T | null
                            // => ...` to lookahead-detect as arrow.
                            if matches!(self.tokens.get(j).map(|s| &s.token), Some(Token::Pipe))
                                && matches!(
                                    self.tokens.get(j + 1).map(|s| &s.token),
                                    Some(Token::Null)
                                )
                            {
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

    /// `function (params): R { body }` / `function NAME(params): R { body }`
    /// in expression position. Re-uses the FnDecl parser shape but emits
    /// an `Expr::ArrowFn` (the optional self-name is dropped — function
    /// expression names bind only inside the body, a feature out-of-
    /// scope for the subset).
    pub(super) fn parse_fn_expr(&mut self) -> Result<ExprId, String> {
        // current token is `function`
        self.pos += 1;
        // P-PARSE.5 → RFC 20260713-generator-fn-value-substrate blade 2:
        // `function*() {...}` generator function expressions parse for
        // real (params / return ann / body) and the ExprId is marked in
        // `ast.gen_fn_exprs`; the `hoist_gen_fn_exprs` AST pass lifts
        // each marked ArrowFn into a top-level `function* __genexpr_N`
        // decl so the existing generator state-machine desugar handles
        // it. (The pre-blade drop-the-body stub silently ran an empty
        // closure — silent-wrong, now gone.)
        let is_generator = matches!(self.peek(), Token::Star);
        if is_generator {
            self.pos += 1;
        }
        // Optional self-name — chunk 796: recorded by ExprId for the
        // NamedEvaluation registry (`.name` / fn-print; ES §15.5.5 —
        // the self-name wins over a binding name). Body-scope
        // self-binding stays out of scope for the subset.
        let self_name = if let Token::Ident(n) = self.peek() {
            let n = n.clone();
            self.pos += 1;
            Some(n)
        } else {
            None
        };
        let (params, destr_lets) = self.parse_param_list()?;
        let mut return_type = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            Some(self.parse_type_ann()?)
        } else {
            None
        };
        // Generator expressions mirror parse_fn's decl-form unwrap:
        // `Generator<T>` / `IterableIterator<T>` / `Iterator<T>` return
        // annotations collapse to the yield type T the state-machine
        // desugar needs (the hoist pass emits the FnDecl with this
        // already-unwrapped ann, so it can't re-run the parser helper).
        if is_generator && let Some(ann) = &return_type {
            let yield_ty = unwrap_generator_return_ann(ann);
            if &yield_ty != ann {
                return_type = Some(yield_ty);
            }
        }
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` after function expression header, got {t:?} at {}",
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
                    "expected `}}` after function expression body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let destr_prefix = destr_lets.len();
        let stmts = if destr_lets.is_empty() {
            stmts
        } else {
            let mut full = destr_lets;
            full.extend(stmts);
            full
        };
        let eid = self.ast.add_expr(Expr::ArrowFn {
            params,
            return_type,
            body: stmts,
        });
        if is_generator {
            self.ast.gen_fn_exprs.insert(
                eid,
                crate::ast::GenFnExprInfo {
                    kind: crate::ast::GenFnExprKind::Generator,
                    destr_prefix,
                },
            );
        } else {
            // RFC 20260717-fnexpr-this-channel knife 1 — the ArrowFn node
            // is a lossy encoding of a function expression: an arrow
            // takes the lexical `this`, a fn-expr binds it at the call
            // site. Record the position so `desugar_fnexpr_this` can give
            // the ones sitting in inline accessor-face positions a
            // `__this` receiver param. (Generator fn-exprs hoist through
            // `hoist_gen_fn_exprs` into decl form instead.)
            self.ast.fn_expr_exprs.insert(eid);
        }
        if let Some(n) = self_name {
            self.ast.fn_expr_self_names.insert(eid, n);
        }
        Ok(eid)
    }

    pub(super) fn parse_arrow_fn(&mut self) -> Result<ExprId, String> {
        // assumes current token is `(`
        self.pos += 1;
        let mut params = Vec::new();
        // V3-18 wedge — destructuring patterns in arrow-fn params,
        // mirror of the parse_fn wedge. `xs.map(([a, b]) => a + b)`
        // is the common shape this unblocks.
        let mut param_destr_lets: Vec<Stmt> = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                if matches!(self.peek(), Token::LBracket | Token::LBrace) {
                    let synth = self.parse_destr_param(&mut param_destr_lets)?;
                    let type_ann = if matches!(self.peek(), Token::Colon) {
                        self.pos += 1;
                        Some(self.parse_type_ann()?)
                    } else {
                        None
                    };
                    // P-PARSE.6 — whole-pattern default on a destr
                    // arrow param: `({a, b} = {a:1, b:2}) => ...`. Per
                    // ES spec §10.2.3 the default fires when the arg
                    // slot is undefined; tora's Param.default plumbs
                    // this through the existing default-arg pipeline,
                    // and the synth binding then carries the
                    // (possibly-defaulted) value into the destr lets.
                    let default = if matches!(self.peek(), Token::Eq) {
                        self.pos += 1;
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    // RFC 20260714-dstr-residual — a whole-pattern
                    // default pins the un-annotated synth param to the
                    // default's inferred type (`= {}` → empty Struct),
                    // and the desugared pattern reads then miss at
                    // lower time ("no field" panic). Force `any` — the
                    // catch-destr precedent: reads route the Any tier
                    // and per-field defaults gate correctly.
                    let type_ann = if type_ann.is_none() && default.is_some() {
                        Some("any".to_string())
                    } else {
                        type_ann
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
                // V3-18 wedge — optional parameter in arrow fn:
                // `(x?: T) => ...`. Same modeling as parse_fn.
                let optional = matches!(self.peek(), Token::Question);
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
                let default = if matches!(self.peek(), Token::Eq) {
                    self.pos += 1;
                    Some(self.parse_expr()?)
                } else {
                    // Note: implicit null default for arrow `(x?: T)`
                    // is not synthesized — closure-call lowering of
                    // Nullable<Number> args is currently broken in
                    // ssa_lower (separate pre-existing bug; tracking).
                    // fn-decl + class-method paths are fine and do
                    // synthesize the null default.
                    None
                };
                params.push(Param {
                    name: pname,
                    type_ann,
                    default,
                    is_rest: false,
                });
                match self.peek() {
                    Token::Comma => {
                        self.pos += 1;
                        // V3-18 wedge — trailing comma in arrow-fn params.
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    }
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
        // V3-18 wedge — prepend destr-param lets to the body, matching
        // the parse_fn wedge.
        let body = if param_destr_lets.is_empty() {
            body
        } else {
            let mut full = param_destr_lets;
            full.extend(body);
            full
        };
        Ok(self.ast.add_expr(Expr::ArrowFn {
            params,
            return_type,
            body,
        }))
    }
}
