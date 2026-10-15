//! The TS this-parameter `this: any` on a function expression.
//!
//! `function (this: any) { this.tag = 1 }` declares the type of the
//! body's `this` and produces no runtime parameter, so
//! `try_consume_this_param` drops it. The receiver passes then type
//! `this` from the position alone: an object-literal field value gets
//! the literal's nominal type, and `this.tag = 1` on a field the
//! literal never declared is refused by the checker (`no field tag on
//! type Struct(...)`) although the program said `any`. The annotation
//! is kept by spelling every `this` of the body as the `as any` cast
//! it means — a cast is type-only, so nothing changes at run time.
//!
//! Only `any`: another annotation keeps today's position-derived type.
//! A nested non-arrow function binds its own `this` and is not
//! entered; an arrow shares the enclosing `this` and is.

use super::*;
use crate::ast::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};

impl Parser<'_> {
    /// The parameter list of a function expression, and whether it
    /// opens with `this: any`.
    pub(super) fn parse_fn_expr_params(&mut self) -> Result<(Vec<Param>, Vec<Stmt>, bool), String> {
        let this_any = self.this_param_is_any();
        let (params, destr_lets) = self.parse_param_list()?;
        Ok((params, destr_lets, this_any))
    }

    /// At the `(` of a parameter list: does it open with `this: any`
    /// followed by `,` or `)`?
    fn this_param_is_any(&self) -> bool {
        let tok = |k: usize| self.tokens.get(self.pos + k).map(|t| &t.token);
        matches!(tok(0), Some(Token::LParen))
            && matches!(tok(1), Some(Token::This))
            && matches!(tok(2), Some(Token::Colon))
            && matches!(tok(3), Some(Token::Ident(n)) if n == "any")
            && matches!(tok(4), Some(Token::Comma | Token::RParen))
    }

    /// With `this_any`, rewrite every `this` the body and the
    /// parameter defaults own into `(this as any)`, in place.
    pub(super) fn cast_own_this_to_any(&mut self, this_any: bool, params: &[Param], body: &[Stmt]) {
        if !this_any {
            return;
        }
        let mut roots: Vec<ExprId> = params.iter().filter_map(|p| p.default).collect();
        let mut stmts: Vec<&Stmt> = body.iter().collect();
        let mut sites: Vec<ExprId> = Vec::new();
        while let Some(s) = stmts.pop() {
            roots.extend(stmt_exprs(s));
            stmts.extend(stmt_children_ref(s));
        }
        while let Some(e) = roots.pop() {
            match self.ast.get_expr(e) {
                Expr::This => sites.push(e),
                Expr::ArrowFn { body, .. } if self.arrow_shares_this(e) => {
                    let mut inner: Vec<&Stmt> = body.iter().collect();
                    while let Some(s) = inner.pop() {
                        roots.extend(stmt_exprs(s));
                        inner.extend(stmt_children_ref(s));
                    }
                }
                _ => roots.extend(expr_children(&self.ast, e)),
            }
        }
        for e in sites {
            let span = self.ast.expr_spans[e.0 as usize];
            let this = self.ast.add_expr(Expr::This);
            self.ast.expr_spans[this.0 as usize] = span;
            self.ast.exprs[e.0 as usize] = Expr::As {
                expr: this,
                ty_ann: "any".into(),
            };
        }
    }

    /// A real arrow — not one of the `ArrowFn` nodes that encode a
    /// function expression, an object-literal method or a generator.
    fn arrow_shares_this(&self, e: ExprId) -> bool {
        !self.ast.fn_expr_exprs.contains(&e)
            && !self.ast.objlit_method_exprs.contains(&e)
            && !self.ast.gen_fn_exprs.contains_key(&e)
    }
}
