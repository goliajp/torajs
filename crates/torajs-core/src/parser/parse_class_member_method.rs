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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_class_member_method_or_ctor(
        &mut self,
        name: &str,
        member_name: String,
        consumed_computed_name: bool,
        explicit_visibility: Option<ast::Visibility>,
        is_readonly: bool,
        is_abstract_method: bool,
        is_static: bool,
        is_async: bool,
        accessor_kind: Option<ast::AccessorKind>,
        fields: &mut Vec<(String, String)>,
        ctor: &mut Option<ClassCtor>,
        methods: &mut Vec<ClassMethod>,
        static_methods: &mut Vec<ClassMethod>,
    ) -> Result<bool, String> {
        // ctor or method
        if !consumed_computed_name {
            self.pos += 1; // consume name
        }
        let is_ctor_branch = member_name == "constructor";
        let (params, promoted_props, destr_lets) = if is_ctor_branch {
            let (p, pr, dl) = self.parse_ctor_param_list()?;
            (p, pr, dl)
        } else {
            let (mut p, dl) = self.parse_param_list()?;
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
                    return Err(format!(
                        "expected `{{` for {member_name} body, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            let mut body = Vec::new();
            while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                body.push(self.parse_stmt()?);
            }
            match self.peek() {
                Token::RBrace => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `}}` to end {member_name} body, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
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
        if member_name == "constructor" {
            if is_static {
                return Err(format!(
                    "`static constructor` is not allowed in class `{name}`"
                ));
            }
            if is_abstract_method {
                return Err(format!(
                    "`abstract constructor` is not allowed in class `{name}`"
                ));
            }
            if ctor.is_some() {
                return Err(format!("duplicate constructor in class `{name}`"));
            }
            // V3-18 wedge — for each TS parameter-property
            // (e.g. `public x: number`), promote to an
            // instance field on the class and prepend
            // `this.<n> = <n>` to the ctor body.
            let mut body = body;
            if !promoted_props.is_empty() {
                let mut prefix: Vec<Stmt> = Vec::new();
                for (idx, vis, rd) in &promoted_props {
                    let p = &params[*idx];
                    let ty_ann = p.type_ann.clone().unwrap_or_else(|| "any".into());
                    fields.push((p.name.clone(), ty_ann));
                    if *vis != ast::Visibility::Public {
                        self.ast
                            .member_visibility
                            .insert((name.to_string(), p.name.clone()), *vis);
                    }
                    if *rd {
                        self.ast
                            .readonly_fields
                            .insert((name.to_string(), p.name.clone()));
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
                body = prefix;
            }
            *ctor = Some(ClassCtor { params, body });
        } else {
            self.finalize_class_method(
                name,
                member_name,
                params,
                return_type,
                body,
                explicit_visibility,
                accessor_kind,
                is_readonly,
                is_abstract_method,
                is_static,
                is_async,
                methods,
                static_methods,
            )?;
        }
        Ok(false)
    }
}
