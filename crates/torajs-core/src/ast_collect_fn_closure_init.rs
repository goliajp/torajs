//! LetDecl-init store-site helpers for
//! [`crate::ast_collect_fn_closure::FnToClosureCollector`]
//! (chunk 789 extraction — the member-chain receiver resolution
//! pushed the collector file past the 500-line limit).

use crate::ast::{Expr, ExprId};

use crate::ast_collect_fn_closure::{FnToClosureCollector, is_fn_like_field_ann};

impl<'a> FnToClosureCollector<'a> {
    /// `const o: T = { k: v, ... }` where `T` resolves to a known
    /// TypeDecl whose field `k` is fn-typed and `v` is a bare
    /// top-FnDecl Ident.
    pub(crate) fn collect_objectlit_field_sites(&mut self, init: ExprId, type_ann: Option<&str>) {
        let Some(ann) = type_ann else { return };
        let Some(field_anns) = self.struct_field_anns.get(ann.trim()) else {
            return;
        };
        if let Expr::ObjectLit { fields } = self.ast.get_expr(init) {
            for (fname, feid) in fields.clone() {
                if field_anns
                    .get(&fname)
                    .is_some_and(|a| is_fn_like_field_ann(a))
                {
                    self.try_mark(feid);
                }
            }
        }
    }

    /// Chunk 733 — mark every bare top-FnDecl Ident element of an
    /// array literal destined for a fn-typed array slot (let-init /
    /// call-arg positions).
    pub(crate) fn mark_array_lit_elems(&mut self, eid: ExprId) {
        if let Expr::Array(els) = self.ast.get_expr(eid) {
            for e in els.clone() {
                self.try_mark(e);
            }
        }
    }

    /// `const f: any = top_fn` (chunk 518) and ObjectLit-field /
    /// Array-element positions inside the `any`-destined init
    /// (chunk 519) — see module doc for why the wrap is needed.
    pub(crate) fn collect_any_init_sites(&mut self, eid: ExprId) {
        if self.try_mark(eid) {
            return;
        }
        match self.ast.get_expr(eid) {
            Expr::ObjectLit { fields } => {
                for (_, feid) in fields.clone() {
                    self.collect_any_init_sites(feid);
                }
            }
            Expr::Array(els) => {
                for e in els.clone() {
                    self.collect_any_init_sites(e);
                }
            }
            _ => {}
        }
    }
}
