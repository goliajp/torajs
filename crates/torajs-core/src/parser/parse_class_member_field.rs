//! `Parser::parse_class_member_field_typed` extracted from
//! `parser/parse_class_decl.rs::parse_class_decl_with_abstract` (chunk
//! 175, 2026-06-28). Sibling of `parse_class_member_method.rs`
//! (chunk 174 split the LParen arm; this splits the Colon arm).
//!
//! Handles the `Some(Token::Colon)` arm: a class field with an explicit
//! type annotation. Two sub-shapes:
//!   * `name: T;`              (instance field)
//!   * `static name: T = init;` (static field, init required)
//!
//! Body verbatim from pre-extract `parse_class_decl_with_abstract`;
//! mechanical rewrites: `name.clone()` → `name.to_string()` (sub-fn
//! takes `&str`), no early-return-with-bool needed (no `continue`
//! in this arm).

use super::*;

impl<'a> Parser<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_class_member_field_typed(
        &mut self,
        name: &str,
        member_name: String,
        consumed_computed_name: bool,
        optional: bool,
        explicit_visibility: Option<ast::Visibility>,
        is_readonly: bool,
        is_abstract_method: bool,
        is_static: bool,
        fields: &mut Vec<(String, String)>,
        static_init: &mut Vec<StaticInit>,
        field_inits: &mut Vec<(String, ExprId)>,
    ) -> Result<(), String> {
        // field declaration. Instance: `name: T;`. Static
        // (M-OO.4): `name: T = init;` — init is required
        // (no constructor to default-init in).
        if is_abstract_method {
            return Err(format!(
                "`abstract` modifier is only valid on methods, not on field `{member_name}` in class `{name}` at {}",
                self.at()
            ));
        }
        if consumed_computed_name {
            self.pos += 1; // consume colon only
        } else {
            // name + colon, plus the `?` of an optional field between
            // them. Every advance here is measured from the same
            // lookahead the member loop matched on, so a token added
            // in the middle has to be counted in both places or the
            // cursor lands on the type and reads it as a member.
            self.pos += 2 + usize::from(optional);
        }
        let ty = self.parse_type_ann()?;
        // §9.2 — `p?: T` IS `p: T | undefined`, which is the same
        // marker the parameter position wraps. Written-out
        // `T | undefined` already arrives wrapped, hence the guard.
        let ty = if optional && !ty.starts_with("__nullable(") {
            format!("__nullable({ty})")
        } else {
            ty
        };
        let visibility = explicit_visibility.unwrap_or(ast::Visibility::Public);
        if visibility != ast::Visibility::Public {
            self.ast
                .member_visibility
                .insert((name.to_string(), member_name.clone()), visibility);
        }
        if is_readonly {
            self.ast
                .readonly_fields
                .insert((name.to_string(), member_name.clone()));
        }
        if is_static {
            match self.peek() {
                Token::Eq => self.pos += 1,
                t => {
                    return Err(format!(
                        "static field `{member_name}` requires an initializer (`= ...`), got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            let init = self.parse_assign()?;
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            static_init.push(StaticInit::Field(ast::StaticField {
                name: member_name,
                type_ann: ty,
                init,
            }));
        } else {
            // V3-18 wedge — accept `name: T = <init>` for
            // instance fields. Init runs in ctor scope
            // before user ctor body executes.
            let init = if matches!(self.peek(), Token::Eq) {
                self.pos += 1;
                Some(self.parse_assign()?)
            } else if optional {
                // §9.2 — an optional field with no initializer holds
                // `undefined`, and saying so explicitly is what gets
                // it there: an absent initializer leaves the slot on
                // its type's zero, which for the pointer-shaped
                // `__nullable(T)` is NULL, i.e. `null`. The bare
                // `p?;` arm below has always synthesized this; the
                // typed spelling has to as well or the two disagree
                // about the same declaration.
                Some(self.ast.add_expr(Expr::Ident("undefined".into())))
            } else {
                None
            };
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            if let Some(init_expr) = init {
                field_inits.push((member_name.clone(), init_expr));
            }
            fields.push((member_name, ty));
        }
        Ok(())
    }

    /// P-SURF S2.29 — bare field declaration `name;` / `name }` (a
    /// `FieldDefinition ;` with no annotation and no initializer, ES
    /// §15.7). The field exists on every instance with the value
    /// `undefined` until written, so it registers as an `any` slot
    /// with an explicit `undefined` init (`c1.x` before any write must
    /// answer `undefined`, not slot garbage). `static name;` mirrors
    /// through the static-field lane with the same synthesized init.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_class_member_field_bare(
        &mut self,
        name: &str,
        member_name: String,
        consumed_computed_name: bool,
        optional: bool,
        explicit_visibility: Option<ast::Visibility>,
        is_readonly: bool,
        is_abstract_method: bool,
        is_static: bool,
        fields: &mut Vec<(String, String)>,
        static_init: &mut Vec<StaticInit>,
        field_inits: &mut Vec<(String, ExprId)>,
    ) -> Result<(), String> {
        if is_abstract_method {
            return Err(format!(
                "`abstract` modifier is only valid on methods, not on field `{member_name}` in class `{name}` at {}",
                self.at()
            ));
        }
        if !consumed_computed_name {
            // the member name, plus the `?` of `p?;`
            self.pos += 1 + usize::from(optional);
        }
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        let visibility = explicit_visibility.unwrap_or(ast::Visibility::Public);
        if visibility != ast::Visibility::Public {
            self.ast
                .member_visibility
                .insert((name.to_string(), member_name.clone()), visibility);
        }
        if is_readonly {
            self.ast
                .readonly_fields
                .insert((name.to_string(), member_name.clone()));
        }
        let undef = self.ast.add_expr(Expr::Ident("undefined".into()));
        if is_static {
            static_init.push(StaticInit::Field(ast::StaticField {
                name: member_name,
                type_ann: "any".into(),
                init: undef,
            }));
        } else {
            fields.push((member_name.clone(), "any".into()));
            field_inits.push((member_name, undef));
        }
        Ok(())
    }
}
