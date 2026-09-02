//! `desugar_classes` per-class default-init synthesis (chunk 182,
//! 2026-06-28).
//!
//! Extracted from `ast/desugar_classes.rs` (Pass 1 sub-section after
//! Phase H.3.b dispatcher emit, before Pass 1.5/1.6 super-call
//! rewriting). Builds:
//!
//!   * `type_alias_fields` — snapshot of every TypeDecl's field
//!     layout, used by `default_init_for_field`'s recursive
//!     class/alias zero-init expansion.
//!   * `class_field_inits` — per-class `[(fname, default_init_expr_id)]`
//!     seeded into the factory's `__this` object literal.
//!   * `class_field_preludes` — per-class hoisted typed-let
//!     declarations (`let __def_arr_<field>: T[] = []`) emitted
//!     before the `__this` literal so empty `T[]` defaults have
//!     ssa-lower-visible element types.
//!
//! Uses the flattened (parent + self) field list from `full_fields`
//! so subclass factories produce a fully-initialized object.
//!
//! Mutates `ast.exprs` via `default_init_for_field`'s `add_expr` calls.

use super::desugar_classes_super::ClassIndexEntry;
use super::*;
use std::collections::HashMap;

pub(super) fn compute_class_field_default_inits(
    ast: &mut Ast,
    class_index: &[ClassIndexEntry],
    full_fields: &HashMap<String, Vec<(PropKey, String)>>,
) -> (
    HashMap<String, Vec<(PropKey, ExprId)>>,
    HashMap<String, Vec<Stmt>>,
) {
    // Build a snapshot of every TypeDecl's field layout. Used by the
    // default-init helper below so a class field whose type is a type
    // alias (`type Step = { value: number, done: boolean }`) gets a
    // structurally-correct zero rather than a Number(0).
    let mut type_alias_fields: HashMap<String, Vec<(PropKey, String)>> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::TypeDecl { name, fields, .. } = s {
            type_alias_fields.insert(name.clone(), fields.clone());
        }
    }
    let combined_fields_map = full_fields.clone();

    let mut class_field_inits: HashMap<String, Vec<(PropKey, ExprId)>> = HashMap::new();
    let mut class_field_preludes: HashMap<String, Vec<Stmt>> = HashMap::new();

    // For each class, build the list of typed default-initializer expressions
    // that the factory will use to seed the `__this` object literal. We use
    // the FLATTENED field list (parent fields + self fields) so subclass
    // factories produce a fully-initialized object.
    //
    // Empty `T[]` defaults need special handling: a bare `[]` in expression
    // position has no inferable element type. We hoist these out into a
    // typed prelude let — `let __def_arr_<field>: T[] = []` — and use the
    // ident as the field init. The let-binding's annotation gives ssa-lower
    // enough context to emit a typed `arr_alloc(0)`.
    //
    // Class- or alias-typed fields recursively expand into a nested
    // ObjectLit of zero-initialized children, looked up via
    // `combined_fields_map` (classes) and `type_alias_fields` (aliases).
    // This is what makes `__Gen_<X>` / `__step_<X>` fields work as
    // class fields on outer iterator classes (J.3 / I.2-inside-gen).
    for (_, cname, _tp, _, _, _, _, _, _) in class_index {
        let combined = full_fields.get(cname).unwrap().clone();
        let mut init_pairs: Vec<(PropKey, ExprId)> = Vec::with_capacity(combined.len());
        let mut prelude: Vec<Stmt> = Vec::new();
        for (fname, fty) in &combined {
            let id = super::default_init_for_field(
                ast,
                fty,
                &combined_fields_map,
                &type_alias_fields,
                &mut prelude,
                cname,
                fname,
                &mut std::collections::HashSet::new(),
            );
            init_pairs.push((fname.clone(), id));
        }
        class_field_inits.insert(cname.clone(), init_pairs);
        class_field_preludes.insert(cname.clone(), prelude);
    }

    (class_field_inits, class_field_preludes)
}
