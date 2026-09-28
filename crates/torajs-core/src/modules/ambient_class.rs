//! A class declaration injected for its shape alone (`import type
//! { C }` with no value walk landing C): everything in it that would
//! EVALUATE goes, since its module never runs. Static initializers
//! are the declaration's own evaluation; a runtime computed member
//! key (`[key]() {}`, `[key] = 1`, `static [key] = 1`) evaluates at
//! class-definition time through the side tables' reify patch and
//! reads a key global the module would have declared. Neither names
//! a member the type can see (a literal key folds to a plain name at
//! parse time), so the members and their side-table rows leave with
//! the initializers. Its member spans index the module's own text,
//! and the registry slices the entry's — the same reset every
//! spliced declaration gets.

use crate::ast::{Ast, Expr, Stmt};

pub(super) fn shape_only(ast: &mut Ast, decl: &mut Stmt) {
    super::splice::clear_injected_spans(decl);
    let Stmt::ClassDecl {
        name,
        decl_id,
        static_init,
        fields,
        ctor,
        methods,
        static_methods,
        ..
    } = decl
    else {
        return;
    };
    static_init.clear();
    let computed = |k: &crate::ast::PropKey| k.starts_with("__ccm_");
    fields.retain(|(k, _)| !computed(k));
    methods.retain(|m| !computed(&m.name));
    static_methods.retain(|m| !computed(&m.name));
    // the ctor prefix writes each computed instance field through
    // its key global: `(this as any)[__ccmk_<C>_<n>] = init`
    if let Some(c) = ctor {
        let before = c.body.len();
        c.body.retain(|s| !is_computed_field_write(ast, s));
        let removed = before - c.body.len();
        if removed > 0
            && let Some(end) = ast.ctor_field_init_end.get_mut(name.as_str())
        {
            *end -= removed;
        }
    }
    // the rows this declaration owns (`class_computed_owner` keys on
    // the sentinel, so a same-named class elsewhere keeps its own)
    let owned: Vec<String> = ast
        .class_computed_owner
        .iter()
        .filter(|(_, ids)| ids.contains(&*decl_id))
        .map(|(s, _)| s.clone())
        .collect();
    ast.class_computed_keys
        .retain(|(_, sentinel), _| !owned.contains(sentinel));
    ast.class_computed_static_fields
        .retain(|(_, sentinel, _)| !owned.contains(sentinel));
    ast.class_computed_instance_fields
        .retain(|(_, sentinel, _)| !owned.contains(sentinel));
    ast.class_computed_static_field_pos
        .retain(|(_, sentinel), _| !owned.contains(sentinel));
}

fn is_computed_field_write(ast: &Ast, s: &Stmt) -> bool {
    let Stmt::Expr(e) = s else {
        return false;
    };
    let Expr::Assign { target, .. } = ast.get_expr(*e) else {
        return false;
    };
    let Expr::Index { index, .. } = ast.get_expr(*target) else {
        return false;
    };
    matches!(ast.get_expr(*index), Expr::Ident(k) if k.starts_with("__ccmk_"))
}
