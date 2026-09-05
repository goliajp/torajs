//! `Parser::parse_class_member_method_or_ctor` extracted from
//! `parser/parse_class_decl.rs::parse_class_decl_with_abstract` (chunk
//! 174, 2026-06-28).
//!
//! Handles the `Some(Token::LParen)` arm of the `match next_tok` in
//! the class-body loop: either a `constructor(...)` (special-cased,
//! stored into `ctor`), a `methodName(...)` (delegated to
//! `finalize_class_method`), or a TS overload signature `name(...): R;`
//! (skip + return `Ok(true)` so the caller's `continue`s the body
//! loop).
//!
//! `Ok(true)` = caller should `continue` outer loop (overload sig).
//! `Ok(false)` = caller falls through to next iteration normally.
//!
//! Body is verbatim from the pre-extract `parse_class_decl_with_abstract`
//! arm; only mechanical rewrites: `name.clone()` → `name.to_string()`
//! (since `name` is `&str` here), `ctor = Some(...)` → `*ctor = Some(...)`.

use super::*;

impl<'a> Parser<'a> {
    /// §15.4.1 MethodDefinition early errors — a getter has no
    /// parameters, a setter exactly one and never a rest parameter.
    /// Shared by the class-member path and the object-literal
    /// accessor shorthand (`object_literal.rs`).
    pub(super) fn reject_accessor_arity(
        &self,
        kind: Option<ast::AccessorKind>,
        params: &[Param],
        ctx: &str,
    ) -> Result<(), String> {
        match kind {
            Some(ast::AccessorKind::Getter) if !params.is_empty() => Err(format!(
                "a getter takes no parameters: `{ctx}` at {} (ES §15.4.1)",
                self.at()
            )),
            Some(ast::AccessorKind::Setter) if params.len() != 1 || params[0].is_rest => {
                Err(format!(
                    "a setter takes exactly one non-rest parameter: `{ctx}` at {} (ES §15.4.1)",
                    self.at()
                ))
            }
            _ => Ok(()),
        }
    }

    /// The "neither `(` nor `:` after the member name" reject.
    pub(super) fn member_shape_err(
        &self,
        member_name: &PropKey,
        t: impl std::fmt::Debug,
    ) -> String {
        format!(
            "expected `(` (method) or `:` (field) after `{}`, got {t:?} at {}",
            member_name.lossy(),
            self.at()
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_class_member_method_or_ctor(
        &mut self,
        name: &str,
        member_name: PropKey,
        consumed_computed_name: bool,
        explicit_visibility: Option<ast::Visibility>,
        is_readonly: bool,
        is_abstract_method: bool,
        is_static: bool,
        is_async: bool,
        accessor_kind: Option<ast::AccessorKind>,
        member_span_start: u32,
        fields: &mut Vec<(PropKey, String)>,
        ctor: &mut Option<ClassCtor>,
        methods: &mut Vec<ClassMethod>,
        static_methods: &mut Vec<ClassMethod>,
    ) -> Result<bool, String> {
        // ctor or method
        if !consumed_computed_name {
            self.pos += 1; // consume name
        }
        // §15.7.1 ClassElementName — `parser/class_element_name.rs`
        // (generator methods call the same judge from their sibling).
        self.reject_class_element_name(
            name,
            member_name.as_str().unwrap_or(""),
            is_static,
            !consumed_computed_name,
            super::class_element_name::ClassElementPos::Method {
                special: accessor_kind.is_some() || is_async,
            },
        )?;
        // A static element spelled `constructor` is an ordinary static
        // member: PrototypePropertyNameList collects only the
        // non-static ones, so it is not the class's constructor and
        // takes the plain-method branch below (rotation 575 — tr used
        // to refuse it, a refusal of programs every engine runs).
        let is_ctor_branch = member_name == "constructor" && !is_static;
        let type_params = self.parse_class_method_type_params(name, is_ctor_branch)?;
        let (params, promoted_props, destr_lets) = if is_ctor_branch {
            let (p, pr, dl) = self.parse_ctor_param_list()?;
            (p, pr, dl)
        } else {
            let (mut p, dl) = self.parse_param_list()?;
            self.finish_formal_params(&p, true)?;
            self.reject_accessor_arity(accessor_kind, &p, &member_name.lossy())?;
            // 刀 1b — method-position default params infer their ann
            // from the default (see param_list.rs).
            self.infer_default_param_anns(&mut p);
            (p, Vec::new(), dl)
        };
        let return_type = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            Some(self.parse_type_ann()?)
        } else {
            None
        };
        // V3-18 wedge — TS class-method overload signature:
        // `methodName(...): R;`. Type-only, terminated by `;`.
        // Skip and continue parsing the class body — the
        // real impl is the trailing same-named decl.
        if !is_abstract_method && matches!(self.peek(), Token::Semi) {
            self.pos += 1;
            return Ok(true);
        }
        // RFC 20260719-fn-tostring-source B3a — span end lands after
        // the body `}` (recorded below once it's consumed). Abstract
        // methods have no body and never exist as fn values → sentinel.
        let mut member_span_end: u32 = 0;
        let body = if is_abstract_method {
            // M-OO.6 — abstract method has no body. ASI per
            // ES spec: `;` is optional when the next token
            // would naturally start a new statement, so
            // accept the next class member directly. Common
            // shape: `abstract area(): number\n  describe()`.
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            Vec::new()
        } else {
            match self.peek() {
                Token::LBrace => self.pos += 1,
                t => {
                    let mn = member_name.lossy();
                    return Err(format!(
                        "expected `{{` for {mn} body, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            // P-SURF S2.9 — the one place `super()` becomes legal
            // (ES §15.7.1: the constructor of a class with an `extends`
            // clause). Every other function-like body clears the flag on
            // entry, so a method's `super()` is refused at the token.
            let saved_super = std::mem::replace(
                &mut self.super_call_allowed,
                is_ctor_branch && self.current_class_has_parent,
            );
            // S2.37 — a static method/accessor body's `this` is the
            // constructor object; see `Parser::static_this_class`.
            let saved_static_this = std::mem::replace(
                &mut self.static_this_class,
                if is_static && !is_ctor_branch {
                    Some(name.to_string())
                } else {
                    None
                },
            );
            // §15.8.1 — await legality follows the member's own
            // asyncness (a constructor is never async).
            let saved_await =
                std::mem::replace(&mut self.await_allowed, is_async && !is_ctor_branch);
            let strict_outer = self.in_strict_fn;
            let mut body = Vec::new();
            while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                match self.parse_stmt() {
                    Ok(s) => {
                        self.arm_strict_directive(&s, &body);
                        body.push(s);
                    }
                    Err(e) => {
                        self.await_allowed = saved_await;
                        self.static_this_class = saved_static_this;
                        return Err(e);
                    }
                }
            }
            self.await_allowed = saved_await;
            self.static_this_class = saved_static_this;
            self.super_call_allowed = saved_super;
            match self.peek() {
                Token::RBrace => {
                    member_span_end = self.tokens.get(self.pos).map(|t| t.span.end).unwrap_or(0);
                    self.pos += 1;
                }
                t => {
                    let mn = member_name.lossy();
                    return Err(format!(
                        "expected `}}` to end {mn} body, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            self.reject_lexical_shadowing_param(&params, &destr_lets, &body)?;
            self.reject_use_strict_with_non_simple_params(&params, &body)?;
            self.finish_fn_body_strict(strict_outer, &params, &mut body)?;
            // V3-18 wedge — prepend destr-param lets when
            // class methods used a binding pattern.
            if destr_lets.is_empty() {
                body
            } else {
                let mut full = destr_lets;
                full.extend(body);
                full
            }
        };
        if is_ctor_branch {
            if is_abstract_method {
                return Err(format!(
                    "`abstract constructor` is not allowed in class `{name}`"
                ));
            }
            if ctor.is_some() {
                return Err(format!("duplicate constructor in class `{name}`"));
            }
            let body = self.promote_ctor_param_props(name, &params, &promoted_props, fields, body);
            self.ast.explicit_ctor_classes.insert(name.to_string());
            *ctor = Some(ClassCtor { params, body });
        } else {
            // Abstract methods keep the (0, 0) sentinel (member_span_end
            // stays 0 and the pair must read as "no source recorded").
            let span = if member_span_end == 0 {
                crate::lexer::Span { start: 0, end: 0 }
            } else {
                crate::lexer::Span {
                    start: member_span_start,
                    end: member_span_end,
                }
            };
            self.finalize_class_method(
                name,
                member_name,
                type_params,
                params,
                return_type,
                body,
                explicit_visibility,
                accessor_kind,
                is_readonly,
                is_abstract_method,
                is_static,
                is_async,
                span,
                methods,
                static_methods,
            )?;
        }
        Ok(false)
    }

    /// V3-18 wedge — for each TS parameter-property (e.g. `public x:
    /// number`), promote to an instance field on the class and prepend
    /// `this.<n> = <n>` to the ctor body.
    /// 398-01 — a method's own type-parameter list, `pair<T>(v: T)`.
    /// Parsed with the standalone-fn list parser and concatenated
    /// after the class-level list on the desugared FnDecl, so
    /// monomorphization needs no new machinery. A constructor cannot
    /// declare its own (TS 1092) — the class-level list is the ctor's.
    fn parse_class_method_type_params(
        &mut self,
        name: &str,
        is_ctor_branch: bool,
    ) -> Result<Vec<String>, String> {
        if !matches!(self.peek(), Token::Lt) {
            return Ok(Vec::new());
        }
        let tp = self.parse_fn_type_params()?;
        if is_ctor_branch {
            return Err(format!(
                "type parameters cannot appear on a constructor declaration in class `{name}` at {}",
                self.at()
            ));
        }
        Ok(tp)
    }

    fn promote_ctor_param_props(
        &mut self,
        name: &str,
        params: &[Param],
        promoted_props: &[(usize, ast::Visibility, bool)],
        fields: &mut Vec<(PropKey, String)>,
        body: Vec<Stmt>,
    ) -> Vec<Stmt> {
        if promoted_props.is_empty() {
            return body;
        }
        let mut prefix: Vec<Stmt> = Vec::new();
        for (idx, vis, rd) in promoted_props {
            let p = &params[*idx];
            let ty_ann = p.type_ann.clone().unwrap_or_else(|| "any".into());
            fields.push((PropKey::from(&p.name), ty_ann));
            if *vis != ast::Visibility::Public {
                self.ast
                    .member_visibility
                    .insert((name.to_string(), PropKey::from(&p.name)), *vis);
            }
            if *rd {
                self.ast
                    .readonly_fields
                    .insert((name.to_string(), PropKey::from(&p.name)));
            }
            let this_ref = self.ast.add_expr(Expr::This);
            let lhs = self.ast.add_expr(Expr::Member {
                obj: this_ref,
                name: p.name.clone(),
            });
            let rhs = self.ast.add_expr(Expr::Ident(p.name.clone()));
            let assign = self.ast.add_expr(Expr::Assign {
                target: lhs,
                value: rhs,
            });
            prefix.push(Stmt::Expr(assign));
        }
        prefix.extend(body);
        prefix
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::Stmt;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    /// Parse `src`, return the recorded span text of the method named
    /// `method` on the first ClassDecl (searching both instance and
    /// static member lists).
    fn class_method_span_text(src: &str, method: &str) -> String {
        let tokens = tokenize(src).expect("tokenize");
        let ast = parse(src, &tokens).expect("parse");
        let span = ast
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::ClassDecl {
                    methods,
                    static_methods,
                    ..
                } => methods
                    .iter()
                    .chain(static_methods.iter())
                    .find(|m| m.name == method)
                    .map(|m| m.span),
                _ => None,
            })
            .expect("a ClassDecl with the method");
        assert!(
            span.start != 0 || span.end != 0,
            "ClassMethod span left at the (0,0) sentinel"
        );
        src[span.start as usize..span.end as usize].to_string()
    }

    #[test]
    fn method_span_covers_name_to_body_brace() {
        let src = "class C {\n  m(a: number): number {\n    return a;\n  }\n}\nnew C();\n";
        assert_eq!(
            class_method_span_text(src, "m"),
            "m(a: number): number {\n    return a;\n  }"
        );
    }

    #[test]
    fn static_method_span_excludes_static_keyword() {
        let src = "class C {\n  static s(): number { return 2; }\n}\nC.s();\n";
        assert_eq!(
            class_method_span_text(src, "s"),
            "s(): number { return 2; }"
        );
    }

    #[test]
    fn getter_span_includes_get_keyword() {
        let src = "class C {\n  get x(): number { return 1; }\n}\nnew C();\n";
        assert_eq!(
            class_method_span_text(src, "x"),
            "get x(): number { return 1; }"
        );
    }

    #[test]
    fn async_method_span_includes_async_keyword() {
        let src = "class C {\n  async go(): Promise<number> { return 1; }\n}\nnew C();\n";
        assert_eq!(
            class_method_span_text(src, "go"),
            "async go(): Promise<number> { return 1; }"
        );
    }

    #[test]
    fn desugared_cm_fndecl_carries_method_span() {
        let src = "class C {\n  m(a: number): number {\n    return a;\n  }\n}\nnew C();\n";
        let tokens = tokenize(src).expect("tokenize");
        let mut ast = parse(src, &tokens).expect("parse");
        crate::ast::desugar_classes(&mut ast);
        let span = ast
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::FnDecl { name, span, .. } if name == "__cm_C__m" => Some(*span),
                _ => None,
            })
            .expect("desugared __cm_C__m FnDecl");
        assert_eq!(
            &src[span.start as usize..span.end as usize],
            "m(a: number): number {\n    return a;\n  }"
        );
    }
}
