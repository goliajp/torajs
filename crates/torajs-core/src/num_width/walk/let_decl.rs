//! The binding-declaration arm of the statement walk.

use super::super::{Analysis, Scope};
use crate::ast::ExprId;

impl Analysis<'_> {
    /// What a binding declaration contributes: the slot's own width
    /// constraint plus every hookup its annotation and initializer
    /// imply. Split out of `walk_stmt` — it is the one arm that
    /// touches five of the analysis's channels at once.
    pub(super) fn walk_let_decl(
        &mut self,
        name: &str,
        type_ann: Option<&str>,
        init: ExprId,
        scope: &Scope,
    ) {
        let w = self.flow(init, scope);
        let key = if scope.fn_name.is_empty() {
            self.keys.global(name)
        } else {
            self.keys.local(scope.fn_name, name)
        };
        // D5 — a binding annotated with a cyclic alias joins the
        // alias's nominal field-width point; F1 — one annotated with
        // a fn-type joins the spelling's signature class.
        if let Some(ann) = type_ann {
            self.alias_ann_union(key, ann);
            self.fnsig_ann_union(key, ann);
            // W-ESC — any-annotated binding is an escape sink
            // (escape.rs).
            self.seed_any_face(key, ann, true);
        }
        // ②.7 — JSON.parse targets: runtime data, every number-domain
        // face seeds F64 (see json_seed.rs).
        if self.is_json_parse(init) {
            self.json_parse_seed(key, type_ann);
        } else if let Some(ann) = type_ann
            && self.expr_types.get(&init) == Some(&crate::check::Type::Any)
        {
            self.any_object_seed(init, key, ann);
        } else if let Some(ann) = type_ann {
            self.any_member_seed(init, key, ann);
        }
        self.add_constraint(key, w);
        self.alias_guarded(key, init, scope);
        self.fn_value_flow(key, init, scope);
        self.walk_expr(init, scope);
    }
}
