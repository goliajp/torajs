//! The statement lists that are scopes of their own: a list's lexical
//! bindings shadow an outer binding of the name from the list's entry.

use super::super::{Param, Stmt, binds_to_params, infer_expr_ann_with};
use super::{Binds, Ctx};

impl<'a> Ctx<'a> {
    /// Walk a statement list that is a scope of its own. Its lexical
    /// bindings shadow any outer one of the name from the list's entry
    /// (§14.2.2), so a closure made before a declaration has run
    /// captures the list's binding: one that shadows a name already
    /// bound starts out with what its declaration will give it, not the
    /// outer binding's annotation.
    pub(super) fn walk_list(&mut self, list: &[Stmt], binds: &mut Binds) {
        self.shadow_list(list, binds);
        for s in list {
            self.walk_stmt(s, binds);
        }
    }

    pub(super) fn shadow_list(&self, list: &[Stmt], binds: &mut Binds) {
        for s in list {
            match s {
                Stmt::LetDecl {
                    name,
                    type_ann,
                    init,
                    is_var: false,
                    ..
                } if binds.contains_key(name) => {
                    let bs: Vec<Param> = binds_to_params(binds);
                    match type_ann.clone().or_else(|| {
                        infer_expr_ann_with(self.exprs, *init, &bs, binds, self.fn_sigs)
                    }) {
                        Some(ann) => binds.insert(name.clone(), ann),
                        None => binds.remove(name),
                    };
                }
                Stmt::Multi(v) => self.shadow_list(v, binds),
                _ => {}
            }
        }
    }
}
