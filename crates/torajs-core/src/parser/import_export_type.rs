//! The TypeScript `type` modifier on import / export declarations and
//! on their specifiers. Its sibling `import_export` answers "what shape
//! is this clause"; this one answers "does a `type` here mark the
//! clause or a specifier as type-only, or is it just a name". Every
//! answer is a lookahead, following TypeScript's own parser: `type`
//! stays an ordinary identifier (`import type from "m"`,
//! `import { type } from "m"`) unless what follows can only be read
//! as a modifier.

use super::*;

impl Parser<'_> {
    fn token_at(&self, k: usize) -> Option<&Token> {
        self.tokens.get(self.pos + k).map(|t| &t.token)
    }

    fn names_something_at(&self, k: usize) -> bool {
        self.token_at(k).is_some_and(|t| {
            matches!(t, Token::Ident(_)) || Self::keyword_property_name(t).is_some()
        })
    }

    /// At `import`'s clause head: `type` followed by a binding, `{` or
    /// `*` makes the whole declaration type-only, except `type from
    /// "m"`, where `type` is the default binding. Consumes the `type`.
    pub(super) fn import_type_head(&mut self) -> bool {
        if !matches!(self.peek(), Token::Type) {
            return false;
        }
        let is_from = |t: Option<&Token>| matches!(t, Some(Token::Ident(n)) if n == "from");
        if is_from(self.token_at(1)) && !is_from(self.token_at(2)) {
            return false;
        }
        let head = matches!(self.token_at(1), Some(Token::LBrace | Token::Star))
            || self.names_something_at(1);
        if head {
            self.pos += 1;
        }
        head
    }

    /// At `export`'s clause head: `type` followed by `{` or `*` makes
    /// the whole declaration type-only (`export type T = …` is a type
    /// alias declaration instead). Consumes the `type`.
    pub(super) fn export_type_head(&mut self) -> bool {
        let head = matches!(self.peek(), Token::Type)
            && matches!(self.token_at(1), Some(Token::LBrace | Token::Star));
        if head {
            self.pos += 1;
        }
        head
    }

    /// At the start of a specifier inside `{ … }`: whether a leading
    /// `type` is a modifier. `type` alone, `type as X` and `type as as`
    /// name the export `type`; `type X`, `type as` and `type as as X`
    /// are type-only specifiers of `X`, `as` and `as` (aliased `X`).
    /// Consumes the modifier.
    pub(super) fn specifier_type_modifier(&mut self) -> bool {
        if !matches!(self.peek(), Token::Type) {
            return false;
        }
        let is_as = |t: Option<&Token>| matches!(t, Some(Token::Ident(n)) if n == "as");
        let modifier = if is_as(self.token_at(1)) {
            if is_as(self.token_at(2)) {
                self.names_something_at(3)
            } else {
                !self.names_something_at(2)
            }
        } else {
            self.names_something_at(1) || matches!(self.token_at(1), Some(Token::String(_)))
        };
        if modifier {
            self.pos += 1;
        }
        modifier
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::Stmt;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn first(src: &str) -> Stmt {
        let tokens = tokenize(src).expect("tokenize");
        let ast = parse(src, &tokens).expect("parse");
        ast.stmts.into_iter().next().expect("a statement")
    }

    fn parse_err(src: &str) -> String {
        let tokens = tokenize(src).expect("tokenize");
        parse(src, &tokens).expect_err("a parse error")
    }

    type ImportSpecs = Vec<(String, Option<String>)>;

    fn import_parts(s: Stmt) -> (bool, ImportSpecs, ImportSpecs) {
        match s {
            Stmt::ImportDecl {
                type_only,
                named,
                type_named,
                ..
            } => (type_only, named, type_named),
            other => panic!("not an import: {other:?}"),
        }
    }

    fn n(s: &str) -> (String, Option<String>) {
        (s.to_string(), None)
    }

    #[test]
    fn whole_import_forms_are_type_only() {
        for src in [
            "import type X from \"./m\";",
            "import type { A, B as C } from \"./m\";",
            "import type * as ns from \"./m\";",
            "import type from from \"./m\";",
        ] {
            assert!(import_parts(first(src)).0, "{src}");
        }
    }

    #[test]
    fn type_as_a_default_binding_stays_a_value() {
        let Stmt::ImportDecl {
            type_only, default, ..
        } = first("import type from \"./m\";")
        else {
            panic!()
        };
        assert!(!type_only);
        assert_eq!(default.as_deref(), Some("type"));
    }

    #[test]
    fn specifier_modifiers_split_type_names_from_values() {
        let (whole, named, typed) =
            import_parts(first("import { type A, B, type C as D } from \"./m\";"));
        assert!(!whole);
        assert_eq!(named, vec![n("B")]);
        assert_eq!(typed, vec![n("A"), ("C".into(), Some("D".into()))]);
    }

    #[test]
    fn type_spelled_specifiers_follow_the_four_lookaheads() {
        let (_, named, typed) = import_parts(first(
            "import { type, type as t, type as, type as as x } from \"./m\";",
        ));
        assert_eq!(named, vec![n("type"), ("type".into(), Some("t".into()))]);
        assert_eq!(typed, vec![n("as"), ("as".into(), Some("x".into()))]);
        let (_, named, typed) = import_parts(first("import { type as as } from \"./m\";"));
        assert_eq!(named, vec![("type".into(), Some("as".into()))]);
        assert!(typed.is_empty());
    }

    #[test]
    fn export_forms_carry_the_modifier() {
        let Stmt::ExportDecl {
            type_only, source, ..
        } = first("export type { A } from \"./m\";")
        else {
            panic!()
        };
        assert!(type_only && source.is_some());
        let Stmt::ExportDecl {
            type_only, star, ..
        } = first("export type * from \"./m\";")
        else {
            panic!()
        };
        assert!(type_only && star.is_some());
        let src = "type A = number; const b = 1; export { type A, b };";
        let tokens = tokenize(src).expect("tokenize");
        let ast = parse(src, &tokens).expect("parse");
        let Some(Stmt::ExportDecl {
            type_only,
            named,
            type_named,
            ..
        }) = ast.stmts.into_iter().nth(2)
        else {
            panic!()
        };
        assert!(!type_only);
        assert_eq!((named, type_named), (vec![n("b")], vec![n("A")]));
    }

    #[test]
    fn modifier_on_both_levels_or_beside_a_default_is_rejected() {
        assert!(parse_err("import type X, { A } from \"./m\";").contains("type-only import"));
        assert!(parse_err("import type { type A } from \"./m\";").contains("`type` modifier"));
        assert!(parse_err("export type { type A };").contains("`type` modifier"));
    }
}
