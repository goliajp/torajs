//! The field arms of the class-member loop — `parse_class_decl_member`'s
//! own split, taken along the seam its module doc already named
//! ("static block / member name / untyped field / field-init
//! finalize"): everything here answers "what does a FIELD declaration
//! turn into", and what is left answers the other two. Verbatim move.

use super::*;

impl<'a> Parser<'a> {
    /// Field-declaration dispatch — the `:` / `=` / `;` / `}` arms
    /// of the member loop share one 10-parameter surface and differ
    /// only in which field parser runs (rotation 240: collapsed out
    /// of `parse_class_decl_with_abstract` when the S2.40 empty-
    /// element skip pushed it past the 200-line fn limit).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_class_member_field_dispatch(
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
        // RFC 20260802 刀 3 后半 — runtime computed FIELD; tail parse
        // in the `parse_class_decl_computed_field.rs` sibling.
        if let Some(sentinel) = member_name.as_str().filter(|s| s.starts_with("__ccm_")) {
            let after = fields.last().map(|(k, _)| k.clone());
            let static_pos = static_init.len();
            return self.parse_computed_field_tail(
                name,
                sentinel.to_string(),
                is_static,
                field_inits,
                (after, static_pos),
            );
        }
        // §15.7.1 ClassElementName, field half. The literal fold above
        // already flipped `consumed_computed_name` for a folded
        // `['...']` key, so that flag is exactly the spec's
        // LiteralPropertyName test; `'constructor'() {}` IS the
        // constructor and never reaches this field dispatch.
        self.reject_class_element_name(
            name,
            member_name.as_str().unwrap_or(""),
            is_static,
            !consumed_computed_name,
            super::class_element_name::ClassElementPos::Field,
        )?;
        let field_inits_mark = field_inits.len();
        let fields_mark = fields.len();
        let static_init_mark = static_init.len();
        let expr_mark = self.ast.exprs.len() as u32;
        // Same lookahead the member loop matched on: the member name
        // is still unconsumed unless the computed-name path already
        // ate it (`name + ]`), so the shape token sits one ahead.
        // ... and the `?` / `!` of `p?: T` sits between the name and
        // that shape token, so it shifts the lookahead exactly as it
        // shifts each parser's own advance below.
        let idx = if consumed_computed_name {
            self.pos
        } else {
            self.pos + 1 + marker.shift()
        };
        match self.tokens.get(idx).map(|t| &t.token) {
            Some(Token::Colon) => self.parse_class_member_field_typed(
                name,
                member_name,
                consumed_computed_name,
                marker,
                explicit_visibility,
                is_readonly,
                is_abstract_method,
                is_static,
                fields,
                static_init,
                field_inits,
            )?,
            Some(Token::Eq) => self.parse_class_member_field_untyped(
                name,
                member_name,
                consumed_computed_name,
                marker,
                explicit_visibility,
                is_readonly,
                is_abstract_method,
                is_static,
                fields,
                static_init,
                field_inits,
            )?,
            _ => self.parse_class_member_field_bare(
                name,
                member_name,
                consumed_computed_name,
                marker,
                explicit_visibility,
                is_readonly,
                is_abstract_method,
                is_static,
                fields,
                static_init,
                field_inits,
            )?,
        }
        // ES §15.7.1 early error — it is a Syntax Error if
        // ContainsArguments of the field's Initializer is true. The
        // three field parsers above push at most one entry onto
        // `field_inits` (instance) or `static_init` (static), so the
        // pre-call marks identify exactly this member's initializer.
        for (_, init) in &field_inits[field_inits_mark..] {
            self.record_field_init_root(*init);
            if super::class_field_early_errors::init_contains_arguments(&self.ast, *init) {
                return Err(format!(
                    "`arguments` is not allowed in a class field initializer in class `{name}` at {} (ES §15.7.1)",
                    self.at()
                ));
            }
        }
        for si in &static_init[static_init_mark..] {
            if let StaticInit::Field(f) = si {
                self.record_field_init_root(f.init);
            }
            if let StaticInit::Field(f) = si
                && super::class_field_early_errors::init_contains_arguments(&self.ast, f.init)
            {
                return Err(format!(
                    "`arguments` is not allowed in a class field initializer in class `{name}` at {} (ES §15.7.1)",
                    self.at()
                ));
            }
        }
        if !is_static {
            self.push_private_field_add(fields, field_inits, fields_mark);
        }
        // The ids this static element's initializer occupies — the
        // positional record `class_static_init_expr_ranges` documents.
        if static_init.len() > static_init_mark {
            self.ast.class_static_init_expr_ranges.push((
                name.to_string(),
                static_init_mark,
                expr_mark,
                self.ast.exprs.len() as u32,
            ));
        }
        Ok(())
    }

    /// Untyped class field with initializer (`name = init` /
    /// `static name = init`, V3-18) — type inferred from a literal
    /// initializer shape. Sibling of the chunk-175 typed-field
    /// split.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_class_member_field_untyped(
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
        // V3-18 wedge — class field with no explicit type
        // ann (`name = init` / `static name = init`). Per
        // TS spec the type is inferred from the init
        // expression; subset infers from literal-shape
        // (Number / String / Boolean / Array of literal /
        // ObjectLit). Other init shapes fall back to
        // requiring an explicit ann.
        if is_abstract_method {
            let mn = member_name.lossy();
            return Err(format!(
                "`abstract` modifier is only valid on methods, not on field `{mn}` in class `{name}` at {}",
                self.at()
            ));
        }
        if consumed_computed_name {
            self.pos += 1; // consume `=` only
        } else {
            // name + `=`, plus the `?` of `p? = init`
            self.pos += 2 + marker.shift();
        }
        // 420-03 — a STATIC field's initializer runs with the class as
        // receiver (§15.7.14). This is the untyped-field arm; the
        // annotated one lives in `parse_class_member_field`.
        let saved_static_this = if is_static {
            self.static_this_class.replace(name.to_string())
        } else {
            self.static_this_class.take()
        };
        let init = if is_static {
            self.parse_field_initializer()
        } else {
            self.parse_instance_field_init()
        };
        self.static_this_class = saved_static_this;
        let init = init?;
        let inferred = match self.ast.get_expr(init) {
            Expr::Number(_) => "number",
            Expr::String(_) => "string",
            Expr::Bool(_) => "boolean",
            // Anything else takes the `any` slot rather than rejecting
            // the program. Real inference needs the initializer's TYPE,
            // which only the checker has — the parser can read shape and
            // nothing more, so scalar literals are the whole of what it
            // can honestly narrow. Refusing the rest bought no safety
            // (the initializer is still parsed and checked; only the
            // SLOT widens) while costing every field whose init is a
            // call, an array, an object, or null. Narrowing these
            // properly belongs in the checker and is registered; doing
            // it here would mean re-implementing type inference on
            // syntax.
            _ => "any",
        };
        let ty = inferred.to_string();
        self.expect_asi_end("a class field")?;
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
        self.name_field_init(name, &member_name, init);
        if is_static {
            static_init.push(StaticInit::Field(ast::StaticField {
                name: member_name,
                type_ann: ty,
                init,
            }));
        } else {
            field_inits.push((member_name.clone(), init));
            fields.push((member_name, ty));
        }
        Ok(())
    }
}
