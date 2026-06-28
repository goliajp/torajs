//! `Parser::parse_stmt` extracted from `parser.rs` (chunk 162).
//!
//! Pre-extract this method was 413 LOC inside `impl Parser` block.
//! Body verbatim moves here as an `impl` block sibling — Rust
//! allows multiple impl blocks for the same type, no re-export
//! needed. Follows the pattern of `parser/class_member.rs` +
//! `parser/object_member.rs` already in this directory.
//!
//! `parse_stmt` is the top-level statement dispatcher — peeks the
//! current token and routes to the corresponding parse_* helper
//! (parse_import / parse_export / parse_block / parse_if /
//! parse_fn / parse_class_decl_with_abstract / parse_while /
//! parse_for / try_parse_for_of / parse_return / parse_throw /
//! parse_try / parse_switch / parse_break / parse_continue /
//! parse_let / parse_var / parse_type_decl / parse_expr_stmt).
//! Body unchanged.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_stmt(&mut self) -> Result<Stmt, String> {
        // V3-18 m1.h.29 — empty statement (`;`). JS spec §13.4
        // ExpressionStatement allows a bare semicolon. Return an
        // empty Block — semantically a no-op, matches what the
        // formatter / lowerer treat as a unit.
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
            return Ok(Stmt::Block(Vec::new()));
        }
        if matches!(self.peek(), Token::Import) {
            return self.parse_import();
        }
        if matches!(self.peek(), Token::Export) {
            return self.parse_export();
        }
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
            return self.parse_fn(false);
        }
        // L.2 — `async function f(...)`. The `async` token is consumed
        // and we set is_async on the resulting FnDecl. desugar_async
        // (post-parse) wraps the body's return value in a Promise and
        // shifts the surface return type from T to Promise<T>.
        if matches!(self.peek(), Token::Async) {
            self.pos += 1;
            if !matches!(self.peek(), Token::Function) {
                return Err(format!(
                    "expected `function` after `async`, got {:?} at {}",
                    self.peek(),
                    self.at()
                ));
            }
            return self.parse_fn(true);
        }
        if matches!(self.peek(), Token::Type) {
            return self.parse_type_decl();
        }
        // V3-18 wedge — `interface X { ... }`. TS-only structural
        // typing declaration; subset desugars to `type X = { ... }`.
        // Contextual keyword: `interface` is just an Ident in the
        // lexer; only treat as a decl when followed by an ident
        // (the interface name).
        if let Token::Ident(s) = self.peek()
            && s == "interface"
            && let Some(next) = self.tokens.get(self.pos + 1)
            && matches!(next.token, Token::Ident(_))
        {
            return self.parse_interface_decl();
        }
        if matches!(self.peek(), Token::Class) {
            return self.parse_class_decl();
        }
        // M-OO.6 — `abstract class C { ... }`. `abstract` is a contextual
        // keyword (just an Ident otherwise) — only treat it as such when
        // followed by `class`.
        if let Token::Ident(s) = self.peek()
            && s == "abstract"
            && let Some(next) = self.tokens.get(self.pos + 1)
            && matches!(next.token, Token::Class)
        {
            self.pos += 1; // consume `abstract`
            return self.parse_class_decl_with_abstract(true, false, false);
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
        if matches!(self.peek(), Token::Yield) {
            // `yield e ;` — Phase J. Parser-level only; the surrounding
            // function must be `function*` or `desugar_generators` will
            // surface this as a typecheck error. Single-arg form only —
            // tr's subset doesn't accept the `yield;` (undefined value)
            // shape.
            self.pos += 1;
            // J.3 — `yield * gen(args);` delegates to an inner generator.
            // Parse-time desugar: same iterator-protocol shape as for-of
            // over a known generator factory, with `yield __step.value`
            // as the loop body.
            if matches!(self.peek(), Token::Star) {
                self.pos += 1;
                let src = self.parse_expr()?;
                if matches!(self.peek(), Token::Semi) {
                    self.pos += 1;
                }
                let (callee_name, yield_ty) = match self.ast.get_expr(src) {
                    Expr::Call { callee, .. } => match self.ast.get_expr(*callee) {
                        Expr::Ident(n) => match self.generator_fns.get(n).cloned() {
                            Some(yt) => (n.clone(), yt),
                            None => {
                                return Err(format!(
                                    "yield* `{n}(...)` — `{n}` is not a known function* \
                                     declaration (J.3 MVP only handles direct generator \
                                     factory calls) at {}",
                                    self.at()
                                ));
                            }
                        },
                        _ => {
                            return Err(format!(
                                "yield* requires a direct call to a function* declaration \
                                 (got non-ident callee) at {}",
                                self.at()
                            ));
                        }
                    },
                    _ => {
                        return Err(format!(
                            "yield* requires a direct call to a function* declaration \
                             (got non-call expr) at {}",
                            self.at()
                        ));
                    }
                };
                let gen_class = format!("__Gen_{callee_name}");
                let step_ty = format!("__step_{callee_name}");
                let id = self.mint_desugar_id();
                let it_name = format!("__yieldstar_it_{id}");
                let step_name = format!("__yieldstar_step_{id}");

                let mut stmts: Vec<Stmt> = Vec::new();
                stmts.push(Stmt::LetDecl {
                    mutable: false,
                    name: it_name.clone(),
                    type_ann: Some(gen_class),
                    init: src,
                    is_var: false,
                });

                let it_ref = self.ast.add_expr(Expr::Ident(it_name));
                let next_member = self.ast.add_expr(Expr::Member {
                    obj: it_ref,
                    name: "next".into(),
                });
                let next_call = self.ast.add_expr(Expr::Call {
                    callee: next_member,
                    args: Vec::new(),
                });
                let step_decl = Stmt::LetDecl {
                    mutable: false,
                    name: step_name.clone(),
                    type_ann: Some(step_ty),
                    init: next_call,
                    is_var: false,
                };

                let step_ref_done = self.ast.add_expr(Expr::Ident(step_name.clone()));
                let done_member = self.ast.add_expr(Expr::Member {
                    obj: step_ref_done,
                    name: "done".into(),
                });
                let done_check = Stmt::If {
                    cond: done_member,
                    then_branch: Box::new(Stmt::Break),
                    else_branch: None,
                };

                let step_ref_value = self.ast.add_expr(Expr::Ident(step_name));
                let value_member = self.ast.add_expr(Expr::Member {
                    obj: step_ref_value,
                    name: "value".into(),
                });
                let _ = yield_ty; // type ann implicit via yield expr
                let yield_stmt = Stmt::Yield(value_member);

                let loop_body = Stmt::Block(vec![step_decl, done_check, yield_stmt]);
                let true_lit = self.ast.add_expr(Expr::Bool(true));
                let while_loop = Stmt::While {
                    cond: true_lit,
                    body: Box::new(loop_body),
                };
                stmts.push(while_loop);
                return Ok(Stmt::Block(stmts));
            }
            let v = self.parse_expr()?;
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            return Ok(Stmt::Yield(v));
        }
        // P2.1 — `var` is parsed identically to `let` here; the
        // difference is the `is_var: true` flag we'll thread into
        // every LetDecl produced from this declaration. The flag
        // drives `desugar_var_hoist` later to lift the declaration
        // to the enclosing fn-body / top-level script (per spec
        // §14.3.2.1 VariableStatement).
        let (mutable, is_var) = match self.peek() {
            Token::Let => (Some(true), false),
            Token::Var => (Some(true), true),
            Token::Const => (Some(false), false),
            _ => (None, false),
        };
        if let Some(mutable) = mutable {
            let kw = if is_var {
                "var"
            } else if mutable {
                "let"
            } else {
                "const"
            };
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
            // V3-18 m1.h.5 — multi-decl `let a, b = 1, c` per spec
            // §14.3.1. Each binding can have its own type ann and
            // optional init; commas separate; final semi closes.
            // Decls are emitted as a Stmt::Multi so subsequent
            // passes see them as a flat statement sequence.
            let mut decls: Vec<Stmt> = Vec::new();
            loop {
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
                // No-init shape: `let x` / `let x: T` (followed by
                // `,` or `;` or — per JS ASI — a known statement-
                // start token on the next line). Const requires an
                // init by spec. T-37-followup-asi: accept Switch /
                // If / For / While / Try / Function / Class / Let /
                // Const / Var / Return / Throw / Break / Continue /
                // Do / RBrace as ASI-implied terminators so test262
                // patterns like `let x\nswitch (x) {...}` parse.
                let next_is_stmt_start = matches!(
                    self.peek(),
                    Token::Switch
                        | Token::If
                        | Token::For
                        | Token::While
                        | Token::Try
                        | Token::Function
                        | Token::Class
                        | Token::Let
                        | Token::Const
                        | Token::Var
                        | Token::Return
                        | Token::Throw
                        | Token::Break
                        | Token::Continue
                        | Token::Do
                        | Token::RBrace
                );
                if matches!(self.peek(), Token::Semi | Token::Comma) || next_is_stmt_start {
                    if !mutable {
                        return Err(format!(
                            "`const {name}` requires an initializer at {}",
                            self.at()
                        ));
                    }
                    let init = self.ast.add_expr(Expr::Uninit);
                    decls.push(Stmt::LetDecl {
                        mutable,
                        name,
                        type_ann,
                        init,
                        is_var,
                    });
                    if matches!(self.peek(), Token::Comma) {
                        self.pos += 1;
                        continue;
                    }
                    // Only consume Semi as terminator; for ASI-style
                    // stmt-start, leave the token for the outer parse.
                    if matches!(self.peek(), Token::Semi) {
                        self.pos += 1;
                    }
                    break;
                }
                match self.peek() {
                    Token::Eq => self.pos += 1,
                    t => return Err(format!("expected `=`, got {t:?} at {}", self.at())),
                }
                // J.4 — `let name(:T)? = yield <expr>;` shape. Only
                // valid as a single-decl for-loop init or assignment;
                // not allowed in the middle of a multi-decl. If the
                // user writes `let x = yield e, y = ...` we fall
                // through to parse_expr which won't accept yield —
                // matches the v0.5 generator semantics.
                if decls.is_empty() && matches!(self.peek(), Token::Yield) {
                    self.pos += 1;
                    let value = self.parse_expr()?;
                    if matches!(self.peek(), Token::Semi) {
                        self.pos += 1;
                    }
                    return Ok(Stmt::YieldInto {
                        var: name,
                        type_ann,
                        value,
                    });
                }
                let init = self.parse_expr()?;
                // P8.5 — narrow-surface class-value alias registration.
                // For const-bindings only, peek the init expr:
                //   (i) `const F = class { ... }` → init is the synth
                //       Ident emitted by parse_primary's Class branch
                //       (`__ClassExpr_<id>`). Register F → that name.
                //   (ii) `const G = F` where F is already an alias →
                //        propagate so G also maps to the underlying
                //        synth class.
                // The map is read by parse_new to rewrite `new F()` /
                // `new G()` into a static factory call against the
                // synth name. let/var bindings and reassignment are
                // intentionally skipped (those need real dynamic-ctor
                // dispatch, parked as L3b).
                if !mutable && !is_var {
                    if let Expr::Ident(init_name) = self.ast.get_expr(init) {
                        if init_name.starts_with("__ClassExpr_") {
                            self.class_value_aliases
                                .insert(name.clone(), init_name.clone());
                        } else if let Some(target) = self.class_value_aliases.get(init_name) {
                            let target = target.clone();
                            self.class_value_aliases.insert(name.clone(), target);
                        }
                    }
                }
                decls.push(Stmt::LetDecl {
                    mutable,
                    name,
                    type_ann,
                    init,
                    is_var,
                });
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                    continue;
                }
                if matches!(self.peek(), Token::Semi) {
                    self.pos += 1;
                }
                break;
            }
            return Ok(if decls.len() == 1 {
                decls.into_iter().next().unwrap()
            } else {
                Stmt::Multi(decls)
            });
        }
        // T-46 — labeled statement (`label: stmt`). JS spec §13.13.
        // tora doesn't track labels for `break label` / `continue label`
        // (those are still parsed as bare Break / Continue), so the
        // minimal handling here is to strip the leading `Ident COLON`
        // chain and parse the inner stmt. Stacked labels
        // (`L1: L2: stmt`) are flattened by the recursive call.
        // Detection: stmt-level `Ident COLON` is unambiguous — the
        // only conflicting expression-level shape (`obj: type` in an
        // object literal / interface) only appears as an Expr context,
        // not as the first two tokens of a Stmt.
        if let Token::Ident(_) = self.peek()
            && let Some(next) = self.tokens.get(self.pos + 1)
            && matches!(next.token, Token::Colon)
        {
            self.pos += 2; // consume label ident + ':'
            return self.parse_stmt();
        }
        let expr = self.parse_expr()?;
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::Expr(expr))
    }
}
