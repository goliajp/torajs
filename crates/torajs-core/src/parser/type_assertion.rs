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
        if let Some(names) = self.try_generic_arrow_params() {
            let arrow = self.parse_arrow_fn()?;
            self.erase_type_params(arrow, &names);
            return Ok(arrow);
        }
        let ty_ann = self.parse_type_ann()?;
        self.consume_type_close("a type assertion")?;
        if matches!(self.peek(), Token::LParen) && self.is_arrow_fn_at_lparen() {
            return Err(format!(
                "unexpected arrow function after the type assertion at {lt_at}"
            ));
        }
        let expr = self.parse_unary()?;
        Ok(self.add_expr_at(start, Expr::As { expr, ty_ann }))
    }

    /// After the `<`: a type parameter list (`T`, `T extends C`,
    /// `T = D`, a trailing comma) closed by `>` and followed by an
    /// arrow's parameter list. Answers the names with the position past
    /// the `>`; anything else restores the position.
    fn try_generic_arrow_params(&mut self) -> Option<Vec<String>> {
        let save = self.pos;
        let mut names = Vec::new();
        let ok = loop {
            let Token::Ident(n) = self.peek() else {
                break false;
            };
            names.push(n.clone());
            self.pos += 1;
            if matches!(self.peek(), Token::Extends) {
                self.pos += 1;
                if self.parse_type_ann().is_err() {
                    break false;
                }
            }
            if matches!(self.peek(), Token::Eq) {
                self.pos += 1;
                if self.parse_type_ann().is_err() {
                    break false;
                }
            }
            if matches!(self.peek(), Token::Comma) {
                self.pos += 1;
            }
            if matches!(self.peek(), Token::Gt) {
                self.pos += 1;
                break matches!(self.peek(), Token::LParen) && self.is_arrow_fn_at_lparen();
            }
        };
        if !ok {
            self.pos = save;
            return None;
        }
        Some(names)
    }

    /// `<T, U extends C = D>` before a function expression's
    /// parameter list, answered as names for [`Self::erase_type_params`];
    /// no list answers none.
    pub(super) fn parse_erased_type_params(&mut self) -> Result<Vec<String>, String> {
        if !matches!(self.peek(), Token::Lt) {
            return Ok(Vec::new());
        }
        self.pos += 1;
        let mut names = Vec::new();
        while let Token::Ident(n) = self.peek() {
            names.push(n.clone());
            self.pos += 1;
            for t in [Token::Extends, Token::Eq] {
                if *self.peek() == t {
                    self.pos += 1;
                    self.parse_type_ann()?;
                }
            }
            if !matches!(self.peek(), Token::Comma) {
                break;
            }
            self.pos += 1;
        }
        self.consume_type_close("type parameters")?;
        Ok(names)
    }

    /// A generic arrow runs as the same arrow without its type
    /// parameters: an annotation that names one is dropped, so the
    /// parameter is typed per call the way an unannotated one is.
    pub(super) fn erase_type_params(&mut self, arrow: ExprId, names: &[String]) {
        let names_one = |ann: &Option<String>| ann.as_deref().is_some_and(|a| names_type(a, names));
        if let Expr::ArrowFn {
            params,
            return_type,
            ..
        } = &mut self.ast.exprs[arrow.0 as usize]
        {
            for p in params.iter_mut() {
                if names_one(&p.type_ann) {
                    p.type_ann = None;
                }
            }
            if names_one(return_type) {
                *return_type = None;
            }
        }
    }
}

/// Does the annotation spell one of `names` as a whole word?
fn names_type(ann: &str, names: &[String]) -> bool {
    ann.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
        .any(|w| names.iter().any(|n| n == w))
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

    fn arrow_param_anns(ast: &crate::ast::Ast) -> Vec<Option<String>> {
        ast.exprs
            .iter()
            .filter_map(|e| match e {
                Expr::ArrowFn { params, .. } => Some(params.iter().map(|p| p.type_ann.clone())),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn generic_arrow_is_not_read_as_an_assertion() {
        let ast = parse("const id = <T>(x: T, n: number): T => x;").unwrap();
        assert!(casts(&ast).is_empty());
        assert_eq!(
            arrow_param_anns(&ast),
            vec![None, Some("number".to_string())]
        );
    }

    #[test]
    fn constrained_and_trailing_comma_type_params_open_an_arrow() {
        for src in [
            "const k = <T extends object>(o: T) => o;",
            "const k = <T,>(o: T) => o;",
            "const k = <T = string, U extends T[] = T[]>(o: T, u: U) => o;",
        ] {
            let ast = parse(src).unwrap();
            assert!(casts(&ast).is_empty(), "{src}");
            assert!(arrow_param_anns(&ast).iter().all(Option::is_none), "{src}");
        }
    }

    #[test]
    fn assertion_of_a_parenthesized_operand_stays_an_assertion() {
        let ast = parse("const c: any = 5; const n = <number>(c);").unwrap();
        assert_eq!(casts(&ast), vec!["number".to_string()]);
    }
}
