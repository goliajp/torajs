//! The statement lists that are scopes of their own: a list's lexical
//! bindings shadow an outer binding of the name from the list's entry.

use super::super::{Param, Stmt, binds_to_params, infer_expr_ann_with};
use super::{Binds, Ctx};

impl<'a> Ctx<'a> {
    /// Walk a statement list that is a scope of its own. Its lexical
    /// bindings shadow any outer one of the name from the list's entry
    /// (§14.2.2), so a closure made before a declaration has run
    /// captures the list's binding: each starts out with what its
    /// declaration will give it, never the annotation of an outer
    /// binding or of an unrelated one the by-name fallback holds.
    pub(super) fn walk_list(&mut self, list: &[Stmt], binds: &mut Binds) {
        let saved = self.enter_list(list, binds);
        for s in list {
            self.walk_stmt(s, binds);
        }
        leave_list(binds, saved);
    }

    /// Shadow `list`'s lexical bindings and answer what `binds` held
    /// for each of their names before, for [`leave_list`].
    pub(super) fn enter_list(&self, list: &[Stmt], binds: &mut Binds) -> Saved {
        let mut saved = Vec::new();
        declared(list, binds, &mut saved);
        self.shadow_list(list, binds);
        saved
    }

    fn shadow_list(&self, list: &[Stmt], binds: &mut Binds) {
        for s in list {
            match s {
                Stmt::LetDecl {
                    name,
                    type_ann,
                    init,
                    is_var: false,
                    ..
                } => {
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

/// The names a list declares, each with what the binds held for it
/// before the list.
pub(super) type Saved = Vec<(String, Option<String>)>;

fn declared(list: &[Stmt], binds: &Binds, out: &mut Saved) {
    for s in list {
        match s {
            Stmt::LetDecl {
                name,
                is_var: false,
                ..
            } => out.push((name.clone(), binds.get(name).cloned())),
            Stmt::Multi(v) => declared(v, binds, out),
            _ => {}
        }
    }
}

/// A list's bindings end with it: the outer binding of each name is
/// the one in force again, so a block's `let` never answers for a
/// closure made after the block.
pub(super) fn leave_list(binds: &mut Binds, saved: Saved) {
    for (name, prev) in saved.into_iter().rev() {
        match prev {
            Some(ann) => binds.insert(name, ann),
            None => binds.remove(&name),
        };
    }
}
