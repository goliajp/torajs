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

/// The one token that may sit between a class field's name and its
/// shape token, and what it means.
///
/// The two markers are mutually exclusive and share exactly one
/// effect — each shifts every downstream cursor by one — so they
/// were, until 563-07, a single `optional: bool`. They differ in
/// the other half: `?` widens the declared type (§9.2 — `p?: T` IS
/// `p: T | undefined`), while TS's definite assignment assertion
/// asserts the field IS assigned and so leaves the type alone.
/// That is the whole of `!`: it is a claim addressed to the type
/// checker and has no runtime face at all.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldMarker {
    /// `p: T` — nothing between the name and the shape token.
    None,
    /// `p?: T` / `p?` / `p? = init`.
    Optional,
    /// `p!: T` — definite assignment assertion.
    Definite,
}

impl FieldMarker {
    /// How far the marker pushes every cursor measured from the
    /// member-name lookahead.
    pub(super) fn shift(self) -> usize {
        usize::from(self != Self::None)
    }

    /// Whether the declared type widens to `T | undefined`.
    pub(super) fn is_optional(self) -> bool {
        self == Self::Optional
    }
}

impl<'a> Parser<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_class_member_field_typed(
        &mut self,
        name: &str,
        member_name: PropKey,
        consumed_computed_name: bool,
        marker: FieldMarker,
        explicit_visibility: Option<ast::Visibility>,
        is_readonly: bool,
        is_abstract_method: bool,
        is_static: bool,
        fields: &mut Vec<(PropKey, String)>,
        static_init: &mut Vec<StaticInit>,
        field_inits: &mut Vec<(PropKey, ExprId)>,
    ) -> Result<(), String> {
        // field declaration. Instance: `name: T;`. Static
        // (M-OO.4): `name: T = init;` — init is required
        // (no constructor to default-init in).
        if is_abstract_method {
            let mn = member_name.lossy();
            return Err(format!(
                "`abstract` modifier is only valid on methods, not on field `{mn}` in class `{name}` at {}",
                self.at()
            ));
        }
        if consumed_computed_name {
            self.pos += 1; // consume colon only
        } else {
            // name + colon, plus the `?` / `!` of a marked field between
            // them. Every advance here is measured from the same
            // lookahead the member loop matched on, so a token added
            // in the middle has to be counted in both places or the
            // cursor lands on the type and reads it as a member.
            self.pos += 2 + marker.shift();
        }
        let mut ty = self.parse_type_ann()?;
        // §9.2 — `p?: T` IS `p: T | undefined`, which is the same
        // marker the parameter position wraps. Written-out
        // `T | undefined` already arrives wrapped, hence the guard.
        if marker.is_optional() && !ty.starts_with("__nullable(") {
            self.ast
                .optional_members
                .plain
                .insert((name.to_string(), member_name.clone()));
            ty = format!("__nullable({ty})");
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
        if is_static {
            let init = if matches!(self.peek(), Token::Eq) {
                self.pos += 1;
                // 420-03 — a static field initializer's `this` is the
                // class object too (§15.7.14 runs it with the class as
                // receiver), so register the token the way a static
                // method body's is.
                let saved_static_this = self.static_this_class.replace(name.to_string());
                let init = self.parse_assign();
                self.static_this_class = saved_static_this;
                let init = init?;
                self.name_field_init(name, &member_name, init);
                init
            } else {
                // §7.3.32 DefineField step 6 — "Else, let initValue be
                // undefined". A static field with no initializer IS
                // defined, and its value is `undefined`. This used to
                // be a parse error, which refuses a program tsc
                // accepts: strictPropertyInitialization checks
                // instance properties only, and the read side agrees
                // — `A.v` types `number` there, unsoundly and on
                // purpose (microsoft/TypeScript#27899).
                //
                // The slot is widened rather than left at `T`, and
                // that is the whole decision. A `number` slot has a
                // NaN sentinel it could carry instead, which keeps
                // the declared type — but the recognition of that
                // sentinel does not survive a function-call boundary
                // yet, so `static g(): number { return C.v }` answers
                // NaN where the spec says undefined. Widening turns
                // that same program into a refusal (the return type
                // mismatch), and a refusal is the honest half of the
                // trade: nothing this declaration admits answers a
                // wrong value.
                //
                // `no_init_type_ann` is the rule already: `let x: T;`
                // with no reachable write gets exactly this treatment
                // one desugar later. The static position was refused
                // before it could reach it.
                ty = crate::ast::uninit_let::no_init_type_ann(&ty);
                self.ast.add_expr(Expr::Ident("undefined".into()))
            };
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
                Some(self.parse_instance_field_init()?)
            } else if marker.is_optional() {
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
                self.name_field_init(name, &member_name, init_expr);
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
        member_name: PropKey,
        consumed_computed_name: bool,
        marker: FieldMarker,
        explicit_visibility: Option<ast::Visibility>,
        is_readonly: bool,
        is_abstract_method: bool,
        is_static: bool,
        fields: &mut Vec<(PropKey, String)>,
        static_init: &mut Vec<StaticInit>,
        field_inits: &mut Vec<(PropKey, ExprId)>,
    ) -> Result<(), String> {
        if is_abstract_method {
            let mn = member_name.lossy();
            return Err(format!(
                "`abstract` modifier is only valid on methods, not on field `{mn}` in class `{name}` at {}",
                self.at()
            ));
        }
        if !consumed_computed_name {
            // the member name, plus the `?` / `!` of `p?;`
            self.pos += 1 + marker.shift();
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
