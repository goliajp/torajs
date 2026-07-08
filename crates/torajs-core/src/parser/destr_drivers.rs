//! `let [a, b, c] = src` / `let { x, y } = src` destructuring drivers (chunk 411).
//!
//! Extracted verbatim from parser.rs — 4 methods that own the top-level
//! shape of let/const array-pattern and object-pattern destructuring:
//! - parse_array_destructuring — array-pattern shape reader
//! - emit_array_destructuring — array-pattern lowering to LetDecl vec
//! - parse_object_destructuring — object-pattern shape reader
//! - emit_object_destructuring — object-pattern lowering to LetDecl vec
//!
//! Delegate to parser/destr_helpers.rs (chunk 410) for the recursive
//! per-element / per-field walker (parse_destr_array_into, etc.).
//! All 4 marked `pub(super)` for cross-module impl-block access.
//! Called only from parser.rs main file. Body unchanged.

use super::*;

impl<'a> Parser<'a> {
    /// `let [a, b, c] = src` → `let __t = src; let a = __t[0]; let b = __t[1]; ...`.
    /// Source is bound once into a `__destr_src_N` temp (skipped if the
    /// source was already an Ident — the alias rule keeps the original
    /// binding the owner). Each pattern entry produces a regular
    /// LetDecl; mem2reg + ssa_lower already optimize the resulting
    /// shape down to direct loads.
    ///
    /// V3-18 wedge — element omission via `,,` (`let [a, , c] = src`)
    /// and trailing rest (`let [head, ...tail] = src`) per ES spec
    /// §13.3.3 / §14.3.3:
    ///   * elision = a `,` token where an element would go; the slot
    ///     is parsed but no LetDecl is emitted (the position counter
    ///     still advances so the next entry reads from the right
    ///     index).
    ///   * rest = `...ident` as the final entry (must be last per
    ///     spec); emitted as `let tail = src.slice(N)` where N is
    ///     the entry count consumed before the rest, reusing
    ///     Array.prototype.slice's existing 1-arg shape.
    pub(super) fn parse_array_destructuring(&mut self, mutable: bool) -> Result<Stmt, String> {
        self.pos += 1; // consume `[`
        // None = elision slot (`,,`); Some((n, default_expr)) = bound
        // name plus optional ES §13.3.3 BindingElement init (default
        // fires when src[i] is undefined; mirrors object destructuring).
        let mut entries: Vec<Option<(String, Option<ExprId>)>> = Vec::new();
        let mut rest_name: Option<String> = None;
        if !matches!(self.peek(), Token::RBracket) {
            loop {
                if matches!(self.peek(), Token::DotDotDot) {
                    self.pos += 1;
                    let n = match self.peek() {
                        Token::Ident(n) => n.clone(),
                        t => {
                            return Err(format!(
                                "expected identifier after `...` in array destructuring, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    };
                    self.pos += 1;
                    rest_name = Some(n);
                    // rest must be the last entry — break to expect `]`.
                    break;
                }
                if matches!(self.peek(), Token::Comma) {
                    // elision: empty slot, advance past the `,` and continue.
                    entries.push(None);
                    self.pos += 1;
                    continue;
                }
                if matches!(self.peek(), Token::RBracket) {
                    break;
                }
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
                let default_expr = if matches!(self.peek(), Token::Eq) {
                    self.pos += 1;
                    Some(self.parse_assign()?)
                } else {
                    None
                };
                entries.push(Some((n, default_expr)));
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
        let stmts = self.emit_array_destructuring(mutable, &entries, rest_name.as_deref(), src);
        // Stmt::Multi flattens at lowering time, so the user-visible
        // lets join the surrounding scope rather than a fresh frame —
        // matches TS semantics where `let [a, b] = src; …; a` works.
        Ok(Stmt::Multi(stmts))
    }

    pub(super) fn emit_array_destructuring(
        &mut self,
        mutable: bool,
        entries: &[Option<(String, Option<ExprId>)>],
        rest_name: Option<&str>,
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
                is_var: false,
            });
        }
        for (i, entry) in entries.iter().enumerate() {
            if let Some((name, default)) = entry {
                let src_ref = self.ast.add_expr(Expr::Ident(src_ref_name.clone()));
                let idx = self.ast.add_expr(Expr::Number(i as f64));
                let elem = self.ast.add_expr(Expr::Index {
                    obj: src_ref,
                    index: idx,
                });
                let init = if let Some(default_eid) = default {
                    // ES §13.3.3 — default fires iff `src[i]` is
                    // undefined. Typed `Array<T>` OOB reads currently
                    // return garbage (not undefined), so the bare
                    // `src[i] !== undefined` gate (used in object
                    // destructuring) selects the wrong branch on
                    // shorter sources. Short-circuit `i < src.length`
                    // first to skip the slot read entirely on OOB,
                    // then the `!== undefined` arm covers the rare
                    // `Array<Nullable<T>>` slot-is-undefined case.
                    // Once OOB-returns-undefined substrate lands the
                    // length guard can be dropped.
                    let src_ref_len = self.ast.add_expr(Expr::Ident(src_ref_name.clone()));
                    let len_mem = self.ast.add_expr(Expr::Member {
                        obj: src_ref_len,
                        name: "length".to_string(),
                    });
                    let idx_lt = self.ast.add_expr(Expr::Number(i as f64));
                    let len_cond = self.ast.add_expr(Expr::BinOp {
                        op: BinOp::Lt,
                        left: idx_lt,
                        right: len_mem,
                    });
                    let src_ref2 = self.ast.add_expr(Expr::Ident(src_ref_name.clone()));
                    let idx2 = self.ast.add_expr(Expr::Number(i as f64));
                    let elem2 = self.ast.add_expr(Expr::Index {
                        obj: src_ref2,
                        index: idx2,
                    });
                    let undef = self.ast.add_expr(Expr::Ident("undefined".to_string()));
                    let undef_cond = self.ast.add_expr(Expr::BinOp {
                        op: BinOp::Neq,
                        left: elem,
                        right: undef,
                    });
                    let combined = self.ast.add_expr(Expr::BinOp {
                        op: BinOp::LAnd,
                        left: len_cond,
                        right: undef_cond,
                    });
                    self.ast.add_expr(Expr::Ternary {
                        cond: combined,
                        then_branch: elem2,
                        else_branch: *default_eid,
                    })
                } else {
                    elem
                };
                stmts.push(Stmt::LetDecl {
                    mutable,
                    name: name.clone(),
                    type_ann: None,
                    init,
                    is_var: false,
                });
            }
            // None = elision: skip, position counter still advances.
        }
        if let Some(rest) = rest_name {
            // `let rest = src.slice(N)` where N is the count of leading
            // entries consumed (including elisions). Array.slice with a
            // single positive arg returns the suffix, exactly per spec.
            let src_ref = self.ast.add_expr(Expr::Ident(src_ref_name.clone()));
            let slice_member = self.ast.add_expr(Expr::Member {
                obj: src_ref,
                name: "slice".to_string(),
            });
            let start = self.ast.add_expr(Expr::Number(entries.len() as f64));
            let slice_call = self.ast.add_expr(Expr::Call {
                callee: slice_member,
                args: vec![start],
            });
            stmts.push(Stmt::LetDecl {
                mutable,
                name: rest.to_string(),
                type_ann: None,
                init: slice_call,
                is_var: false,
            });
        }
        stmts
    }

    /// `let { x, y } = src` → `let __t = src; let x = __t.x; let y = __t.y;`.
    /// Renaming `let { x: foo, y: bar } = src` rebinds to foo / bar.
    /// Same source-binding rule as array destructuring.
    pub(super) fn parse_object_destructuring(&mut self, mutable: bool) -> Result<Stmt, String> {
        self.pos += 1; // consume `{`
        // (field_name, bound_as, default_expr) — default is ES §13.3.3
        // BindingElement init: `{ a = 10 }` / `{ a: aa = 99 }` populates
        // the bound name from `src.field` when present, else from
        // `default_expr`. The default-arm path is gated to undefined
        // (not null) per spec — same semantics as fn param defaults.
        let mut entries: Vec<(String, String, Option<ExprId>)> = Vec::new();
        let mut rest_name: Option<String> = None;
        if !matches!(self.peek(), Token::RBrace) {
            loop {
                // ES §14.3.3 — `...rest` binds a fresh object holding
                // the source's remaining own enumerable properties.
                // Must be the final entry (no trailing comma after).
                if matches!(self.peek(), Token::DotDotDot) {
                    self.pos += 1;
                    let n = match self.peek() {
                        Token::Ident(n) => n.clone(),
                        t => {
                            return Err(format!(
                                "expected identifier after `...` in object destructuring, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    };
                    self.pos += 1;
                    rest_name = Some(n);
                    break;
                }
                // V3-18 wedge — accept reserved-word tokens as
                // destructuring field names (the access-side
                // counterpart of the obj-literal wedge). Reserved-
                // word fields require explicit `field: name` rename
                // since the bound binding name itself can't be a
                // reserved word.
                let (field, field_is_kw) = match self.peek() {
                    Token::Ident(n) => (n.clone(), false),
                    t if Self::keyword_property_name(t).is_some() => {
                        (Self::keyword_property_name(t).unwrap().to_string(), true)
                    }
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
                    if field_is_kw {
                        return Err(format!(
                            "destructuring field `{field}` is a reserved word; use `{{ {field}: <binding> }}` to rename at {}",
                            self.at()
                        ));
                    }
                    field.clone()
                };
                let default_expr = if matches!(self.peek(), Token::Eq) {
                    self.pos += 1;
                    Some(self.parse_assign()?)
                } else {
                    None
                };
                entries.push((field, bound, default_expr));
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
        let mut stmts = self.emit_object_destructuring(mutable, &entries, src);
        if let Some(rest) = rest_name {
            // `let rest = { ...src minus destructured keys }` — the
            // omit set rides in the spread sentinel's name
            // (`__spread_omit__:<comma-joined>`); the ObjectLit
            // checker and lowering unfold skip the named keys, so
            // `rest` types as the source struct minus those fields.
            // emit_object_destructuring bound a `__destr_src_N` temp
            // for every non-Ident source, so the source reference is
            // either that temp or the original Ident.
            let src_ref_name = match stmts.first() {
                Some(Stmt::LetDecl { name, .. }) if name.starts_with("__destr_src_") => {
                    name.clone()
                }
                _ => match self.ast.get_expr(src) {
                    Expr::Ident(n) => n.clone(),
                    e => panic!("object destructuring rest: unbound non-ident source {e:?}"),
                },
            };
            let omit: Vec<&str> = entries.iter().map(|(f, _, _)| f.as_str()).collect();
            let sentinel = format!("__spread_omit__:{}", omit.join(","));
            let src_ref = self.ast.add_expr(Expr::Ident(src_ref_name));
            let obj = self.ast.add_expr(Expr::ObjectLit {
                fields: vec![(sentinel, src_ref)],
            });
            stmts.push(Stmt::LetDecl {
                mutable,
                name: rest,
                type_ann: None,
                init: obj,
                is_var: false,
            });
        }
        Ok(Stmt::Multi(stmts))
    }

    pub(super) fn emit_object_destructuring(
        &mut self,
        mutable: bool,
        entries: &[(String, String, Option<ExprId>)],
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
                is_var: false,
            });
        }
        for (field, bound, default) in entries {
            let src_ref = self.ast.add_expr(Expr::Ident(src_ref_name.clone()));
            let mem = self.ast.add_expr(Expr::Member {
                obj: src_ref,
                name: field.clone(),
            });
            let init = if let Some(default_eid) = default {
                // ES §13.3.3 — default fires iff the destructured slot
                // is undefined (null/zero/empty-string do NOT). Emit
                // `(src.field !== undefined) ? src.field : default`.
                // `src.field` is evaluated twice; for plain ObjectLit
                // sources this has no observable cost. A getter-bearing
                // source would re-run the accessor — acceptable for the
                // narrow shape and consistent with how tora emits
                // `?:` elsewhere; an IIFE / temp-let lowering is a
                // future ergonomics polish.
                let src_ref2 = self.ast.add_expr(Expr::Ident(src_ref_name.clone()));
                let mem2 = self.ast.add_expr(Expr::Member {
                    obj: src_ref2,
                    name: field.clone(),
                });
                let undef = self.ast.add_expr(Expr::Ident("undefined".to_string()));
                let cond = self.ast.add_expr(Expr::BinOp {
                    op: BinOp::Neq,
                    left: mem,
                    right: undef,
                });
                self.ast.add_expr(Expr::Ternary {
                    cond,
                    then_branch: mem2,
                    else_branch: *default_eid,
                })
            } else {
                mem
            };
            stmts.push(Stmt::LetDecl {
                mutable,
                name: bound.clone(),
                type_ann: None,
                init,
                is_var: false,
            });
        }
        stmts
    }
}
