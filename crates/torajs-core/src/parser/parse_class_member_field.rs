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
            self.pos += 2; // consume name + colon
        }
        let ty = self.parse_type_ann()?;
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
}
