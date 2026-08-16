//! Runtime-computed class FIELD parse (RFC 20260802 刀 3 后半) —
//! carved out of `parse_class_decl_member.rs` when the dispatch arm
//! pushed the file past the 500-line cap.
//!
//! The key evaluated once at class-definition time (pass 3 emits
//! `let __ccmk_<C>_<n> = <key>` at the class-decl position); the
//! member NEVER enters the static struct layout — an instance field
//! becomes a ctor-prefix keyed write into the expando dict (RFC
//! 20260714 blade 2), a static one a keyed store onto the class
//! object. `[k]: T = v` type annotations are consumed and erased
//! (the value lands in an `any`-keyed slot either way).

use super::*;

impl<'a> Parser<'a> {
    /// Parse the `[: T]? [= init]? ;?` tail of a computed field whose
    /// name already minted a `__ccm_<n>__` sentinel. Instance inits
    /// ride `field_inits` under the sentinel (the ctor-prefix
    /// injection rewrites them to keyed writes); static ones go to
    /// the `class_computed_static_fields` side table.
    pub(super) fn parse_computed_field_tail(
        &mut self,
        name: &str,
        member_name: String,
        is_static: bool,
        field_inits: &mut Vec<(String, ExprId)>,
    ) -> Result<(), String> {
        if matches!(self.peek(), Token::Colon) {
            self.pos += 1; // consume `:`
            self.parse_type_ann()?;
        }
        let init = if matches!(self.peek(), Token::Eq) {
            self.pos += 1; // consume `=`
            // 420-03 — a STATIC field's initializer runs with the class
            // as receiver (§15.7.14); an instance one runs in ctor
            // scope, where `this` is already the instance.
            let saved_static_this = if is_static {
                std::mem::replace(&mut self.static_this_class, Some(name.to_string()))
            } else {
                self.static_this_class.take()
            };
            let e = self.parse_assign();
            self.static_this_class = saved_static_this;
            let e = e?;
            if super::class_field_early_errors::init_contains_arguments(&self.ast, e) {
                return Err(format!(
                    "`arguments` is not allowed in a class field initializer in class `{name}` at {} (ES §15.7.1)",
                    self.at()
                ));
            }
            e
        } else {
            self.ast.add_expr(Expr::Ident("undefined".into()))
        };
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        if is_static {
            self.ast
                .class_computed_static_fields
                .push((name.to_string(), member_name, init));
        } else {
            field_inits.push((member_name, init));
        }
        Ok(())
    }

    /// The ctor-prefix assignment TARGET for a sentinel field —
    /// `(this as any)[__ccmk_<C>_<n>]`: the key was evaluated once at
    /// class-definition time into the module global (pass 3), the
    /// init re-evaluates per construction, in declaration order with
    /// the named fields.
    pub(super) fn computed_field_init_lhs(
        &mut self,
        class_name: &str,
        sentinel: &str,
        this_ref: ExprId,
    ) -> ExprId {
        let n = sentinel
            .strip_prefix("__ccm_")
            .map(|r| r.trim_end_matches('_'))
            .expect("caller checked the sentinel prefix");
        let this_any = self.ast.add_expr(Expr::As {
            expr: this_ref,
            ty_ann: "any".into(),
        });
        let key_ref = self
            .ast
            .add_expr(Expr::Ident(format!("__ccmk_{class_name}_{n}")));
        self.ast.add_expr(Expr::Index {
            obj: this_any,
            index: key_ref,
        })
    }
}
