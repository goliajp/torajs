//! The TS angle-bracket type assertion `<T>expr`, the prefix spelling of
//! `expr as T`: the same cast for the checker, identity at run time.

use super::*;

impl Parser<'_> {
    /// `<T>operand` at a unary position, the `<` at `self.pos`. A `<`
    /// there opens either this assertion or a generic arrow
    /// (`<T>(x: T) => x`); TypeScript reads it as the arrow whenever the
    /// parenthesized group is followed by one.
    pub(super) fn parse_angle_assertion(&mut self) -> Result<ExprId, String> {
        let start = self.pos;
        let lt_at = self.at();
        self.pos += 1;
        if matches!(self.peek(), Token::Const)
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.token),
                Some(Token::Gt)
            )
        {
            // `<const>[1, 2]`: a const assertion, identity like `as const`
            self.pos += 2;
            return self.parse_unary();
        }
        let ty_ann = self.parse_type_ann()?;
        self.consume_type_close("a type assertion")?;
        if matches!(self.peek(), Token::LParen) && self.is_arrow_fn_at_lparen() {
            return Err(format!(
                "generic arrow functions are not supported yet at {lt_at}"
            ));
        }
        let expr = self.parse_unary()?;
        Ok(self.add_expr_at(start, Expr::As { expr, ty_ann }))
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::Expr;

    fn parse(src: &str) -> Result<crate::ast::Ast, String> {
        let tokens = crate::lexer::tokenize(src).expect("tokenize");
        crate::parser::parse(src, &tokens)
    }

    fn casts(ast: &crate::ast::Ast) -> Vec<String> {
        ast.exprs
            .iter()
            .filter_map(|e| match e {
                Expr::As { ty_ann, .. } => Some(ty_ann.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn prefix_assertion_is_an_as_cast() {
        let ast =
            parse("const c: any = 5; const n = <number>c; const m = <Array<number>>c;").unwrap();
        assert_eq!(
            casts(&ast),
            vec!["number".to_string(), "Array<number>".to_string()]
        );
    }

    #[test]
    fn assertion_binds_tighter_than_binary_ops() {
        let ast = parse("const c: any = 5; const n = <number>c + 1;").unwrap();
        let add = ast.exprs.iter().find_map(|e| match e {
            Expr::BinOp { left, .. } => Some(*left),
            _ => None,
        });
        assert!(matches!(ast.get_expr(add.unwrap()), Expr::As { .. }));
    }

    #[test]
    fn const_assertion_is_identity() {
        let ast = parse("const a = <const>[1, 2];").unwrap();
        assert!(casts(&ast).is_empty());
    }

    #[test]
    fn generic_arrow_is_not_read_as_an_assertion() {
        let err = parse("const id = <T>(x: T) => x;").unwrap_err();
        assert!(err.contains("generic arrow"), "{err}");
    }
}
