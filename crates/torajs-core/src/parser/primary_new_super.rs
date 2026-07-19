//! `super` / `new` expression arms (chunk 421).
//!
//! Extracted verbatim (dedented one level) from parse_primary
//! (parser.rs) — the two largest match arms:
//! - parse_primary_super — `super(args)` ctor call, `super.m(args)`
//!   parent-method call (encoded as `__supercall__<m>` marker ident)
//! - parse_primary_new — `new C(args)` incl. `new.target`,
//!   `new class {...}()`, `new (expr)()`, type-arg skip, and the
//!   no-parens `new Foo` form
//!
//! Both assume the cursor is at the `super` / `new` token; the match
//! arms in parse_primary delegate here. Bodies unchanged.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_primary_super(&mut self) -> Result<ExprId, String> {
        // `super(args)` — only valid inside a subclass ctor; the
        // desugar pass enforces that and rewrites to a Call to
        // `__cm_<Parent>__ctor(__this, args)`.
        // V3-18 wedge — `super.<method>(args)` (explicit
        // parent-method call): encoded as a Call to a marker
        // ident `__supercall__<methodname>`; desugar_classes
        // rewrites it to `__cm_<Parent>__<m>(__this, args)`
        // using the surrounding class's parent.
        self.pos += 1;
        if matches!(self.peek(), Token::Dot) {
            self.pos += 1;
            let m_name = match self.peek() {
                Token::Ident(n) => n.clone(),
                t => {
                    return Err(format!(
                        "expected method name after `super.`, got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            self.pos += 1;
            match self.peek() {
                Token::LParen => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `(` after `super.{m_name}`, got {t:?} at {}",
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
            let callee = self
                .ast
                .add_expr(Expr::Ident(format!("__supercall__{m_name}")));
            return Ok(self.ast.add_expr(Expr::Call { callee, args }));
        }
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

    pub(super) fn parse_primary_new(&mut self) -> Result<ExprId, String> {
        // `new ClassName(args)` — type args / generic ctors not yet
        // supported; that's M5.2 alongside extends.
        self.pos += 1;
        // P4.5 — `new.target` meta-property. Spec §13.3.10:
        // evaluates to the [[NewTarget]] of the current
        // execution context. Recognized at the parser layer
        // because `new` followed by `.` would otherwise hit the
        // class-name error path.
        if matches!(self.peek(), Token::Dot) {
            self.pos += 1;
            match self.peek() {
                Token::Ident(n) if n == "target" => {
                    self.pos += 1;
                    return Ok(self.ast.add_expr(Expr::NewTarget));
                }
                t => {
                    return Err(format!(
                        "expected `target` after `new.`, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        let class_name = match self.peek() {
            Token::Ident(n) => {
                let n = n.clone();
                self.pos += 1;
                // P8.5 — narrow alias resolution. If `new F()`
                // and F is a const-bound class expression,
                // rewrite to `new __ClassExpr_<id>()` so the
                // static-factory path resolves to the synth
                // class's `__new___ClassExpr_<id>`. Avoids a
                // dynamic-ctor-dispatch substrate change.
                self.class_value_aliases.get(&n).cloned().unwrap_or(n)
            }
            // P8.5 — `new class { ... }(args)` /
            // `new class Foo { ... }(args)`. Parse the class
            // expression inline as a ClassDecl with synth name,
            // buffer to synth_classes, and use the synth name
            // as the new target. parse_class_decl_with_abstract
            // consumes `class` + body itself.
            Token::Class => {
                let stmt = self.parse_class_decl_with_abstract(false, true, true)?;
                let cls_name = match &stmt {
                    Stmt::ClassDecl { name, .. } => name.clone(),
                    _ => unreachable!(),
                };
                self.synth_classes.push(stmt);
                cls_name
            }
            // P8.5 — `new (expr)(args)` per ES spec §13.3.5
            // NewExpression: the callee may be a parenthesized
            // expression. tora handles the case where the inner
            // expression resolves to an Ident — which covers
            // `new (class { ... })()` (the class-expression
            // branch in parse_primary emits an Ident at that
            // inner site) and `new (SomeClass)()`. Non-Ident
            // callees (dynamic ctor invocation through a
            // computed value) require runtime constructor
            // dispatch — V3-later.
            Token::LParen => {
                self.pos += 1;
                let inner = self.parse_expr()?;
                match self.peek() {
                    Token::RParen => self.pos += 1,
                    t => {
                        return Err(format!(
                            "expected `)` after `new (...`, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
                match self.ast.get_expr(inner) {
                    Expr::Ident(n) => n.clone(),
                    other => {
                        return Err(format!(
                            "new (expr) callee must resolve to a class ident (got \
                                 {other:?}) at {}",
                            self.at()
                        ));
                    }
                }
            }
            t => {
                return Err(format!(
                    "expected class name after `new`, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        // Explicit TS instantiation on built-in generics:
        // `new Set<number>()` / `new Map<string, number>()`. Parsed
        // into flat ann spellings and carried on `Expr::New.type_args`
        // (callback-param seeding reads the container's element types
        // from here); no mono-instantiation happens downstream yet.
        // The `type_ann_depth` guard keeps the spellings out of
        // `type_ann_spans` — fn_source_erase never spliced the old
        // skip form either, so toString output stays unchanged.
        let mut type_args: Vec<String> = Vec::new();
        if matches!(self.peek(), Token::Lt) {
            self.pos += 1;
            self.type_ann_depth += 1;
            let parsed = self.parse_type_args_list();
            self.type_ann_depth -= 1;
            type_args = parsed?;
        }
        // V3-18 m1.h.22 — JS spec §13.3.5 NewExpression
        // permits `new Foo` (no parens), equivalent to
        // `new Foo()`. Test262 uses both forms; the no-
        // parens form previously hard-rejected.
        let has_parens = matches!(self.peek(), Token::LParen);
        if has_parens {
            self.pos += 1;
        }
        // Ctor args ride the same spread-aware arg parser as plain
        // calls (chunk 684): `...` wraps in Expr::Spread, a static
        // literal spread (`new C(...[1, 2])`) folds to fixed args
        // here, and a dynamic trailing spread (`new C(...arr)`) is
        // desugared by `apply_spread_args`' New arm.
        let mut args: Vec<ExprId> = Vec::new();
        if has_parens && !matches!(self.peek(), Token::RParen) {
            args.push(self.parse_call_arg()?);
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                args.push(self.parse_call_arg()?);
            }
        }
        if has_parens {
            match self.peek() {
                Token::RParen => self.pos += 1,
                t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
            }
        }
        let args = self.fold_static_spread(args);
        Ok(self.ast.add_expr(Expr::New {
            class_name,
            args,
            type_args,
        }))
    }
}
