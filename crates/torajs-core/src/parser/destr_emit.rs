//! Declaration-position destructuring — the bind EMITTER half of
//! [`super::destr_shape`], split from it when the `is_var` threading
//! pushed that file over the 500-line limit.
//!
//! The boundary is the one the file already had: [`super::destr_shape`]
//! answers "what pattern is this" and hands back a `PatShape`, and this
//! answers "what statements does it become". They meet only at
//! `PatShape`, and the statement driver — which asks both questions in
//! turn — stays with the reader.
//!
//! Two invariants live here rather than in either caller:
//!
//! - A pattern's own temporaries (`__ary_src_N` / `__destr_src_N`) are
//!   always lexical. Only the names the SOURCE wrote take the
//!   declaration's `is_var`, because only those are what §14.3.2
//!   hoists.
//! - Load recipes are the shared `dstra_elem_load` / `dstra_field_load`
//!   (array defaults carry the length-guard, object defaults the
//!   undefined-gate), so §13.3.3 default semantics cannot drift
//!   between the declaration and assignment lanes.

use super::destr_shape::{AryElem, ObjBinding, ObjField, PatShape, RestShape};
use super::*;

impl Parser<'_> {
    /// Recursive bind emitter — one `LetDecl` per simple slot, a
    /// fresh temp + recursion per nested slot. Load recipes are the
    /// shared `dstra_elem_load` / `dstra_field_load` (array defaults
    /// carry the length-guard, object defaults the undefined-gate).
    pub(super) fn emit_pattern_binds(
        &mut self,
        pat: &PatShape,
        src_expr: ExprId,
        mutable: bool,
        is_var: bool,
        out: &mut Vec<Stmt>,
    ) {
        match pat {
            PatShape::Ary { elems, rest } => {
                self.emit_ary_binds(elems, rest, src_expr, mutable, is_var, out)
            }
            PatShape::Obj { fields, rest } => {
                self.emit_obj_binds(fields, rest, src_expr, mutable, is_var, out)
            }
        }
    }

    fn emit_ary_binds(
        &mut self,
        elems: &[AryElem],
        rest: &Option<RestShape>,
        src_expr: ExprId,
        mutable: bool,
        is_var: bool,
        out: &mut Vec<Stmt>,
    ) {
        // RFC 20260714-dstr-residual blade 3 — array patterns always
        // read through their own group temp (the checker retypes it
        // to `Array<Any>` on non-indexable sources; the group entry
        // keeps a short source's past-end slots reading `undefined`).
        let id = self.mint_desugar_id();
        let src_name = format!("__ary_src_{id}");
        let limit: i64 = if rest.is_some() {
            -1
        } else {
            elems.len() as i64
        };
        self.ast.ary_destr_groups.insert(src_expr, limit);
        out.push(Stmt::LetDecl {
            mutable: false,
            name: src_name.clone(),
            type_ann: None,
            init: src_expr,
            is_var: false,
        });
        for (i, elem) in elems.iter().enumerate() {
            match elem {
                AryElem::Elide => {}
                AryElem::Bind { name, default } => {
                    if let Some(d) = default {
                        self.record_dstr_default_name(*d, name);
                    }
                    let init = self.dstra_elem_load(&src_name, i, *default);
                    out.push(Stmt::LetDecl {
                        mutable,
                        name: name.clone(),
                        type_ann: None,
                        init,
                        is_var,
                    });
                }
                AryElem::Nested { pat, default } => {
                    let loaded = self.dstra_elem_load(&src_name, i, *default);
                    self.emit_pattern_binds(pat, loaded, mutable, is_var, out);
                }
            }
        }
        if let Some(r) = rest {
            let src_ref = self.ast.add_expr(Expr::Ident(src_name));
            let slice_m = self.ast.add_expr(Expr::Member {
                obj: src_ref,
                name: "slice".into(),
            });
            let start = self.ast.add_expr(Expr::Number(elems.len() as f64));
            let tail = self.ast.add_expr(Expr::Call {
                callee: slice_m,
                args: vec![start],
            });
            match r {
                RestShape::Bind(rest_name) => {
                    out.push(Stmt::LetDecl {
                        mutable,
                        name: rest_name.clone(),
                        type_ann: None,
                        init: tail,
                        is_var,
                    });
                }
                // `[...[x, y]]` — the collected tail array is itself
                // the nested pattern's source; the recursion hoists
                // it into its own group temp like any other source.
                RestShape::Nested(pat) => {
                    self.emit_pattern_binds(pat, tail, mutable, is_var, out);
                }
            }
        }
    }

    fn emit_obj_binds(
        &mut self,
        fields: &[ObjField],
        rest: &Option<String>,
        src_expr: ExprId,
        mutable: bool,
        is_var: bool,
        out: &mut Vec<Stmt>,
    ) {
        // Ident sources stay the owner (no temp); everything else
        // hoists into `__destr_src_N` — the statement driver's
        // long-standing alias rule.
        let src_name = if let Expr::Ident(n) = self.ast.get_expr(src_expr) {
            n.clone()
        } else {
            let id = self.mint_desugar_id();
            let name = format!("__destr_src_{id}");
            out.push(Stmt::LetDecl {
                mutable: false,
                name: name.clone(),
                type_ann: None,
                init: src_expr,
                is_var: false,
            });
            name
        };
        // §13.3.3.5 RequireObjectCoercible — null / undefined source
        // throws even for `{}`.
        let guard = self.emit_object_coercible_guard(&src_name);
        out.push(guard);
        for ObjField { field, binding } in fields {
            match binding {
                ObjBinding::Bind { name, default } => {
                    if let Some(d) = default {
                        self.record_dstr_default_name(*d, name);
                    }
                    let init = self.dstra_field_load(&src_name, field, *default);
                    out.push(Stmt::LetDecl {
                        mutable,
                        name: name.clone(),
                        type_ann: None,
                        init,
                        is_var,
                    });
                }
                ObjBinding::Nested { pat, default } => {
                    let loaded = self.dstra_field_load(&src_name, field, *default);
                    self.emit_pattern_binds(pat, loaded, mutable, is_var, out);
                }
            }
        }
        if let Some(rest_name) = rest {
            let omit: Vec<&str> = fields.iter().map(|f| f.field.as_str()).collect();
            let bind = self.emit_obj_rest_let(&src_name, &omit, rest_name, mutable, is_var);
            out.push(bind);
        }
    }

    /// ES §14.3.3.1 RestBindingInitialization for object patterns —
    /// `{ a, b, ...rest }` binds `rest` to a fresh object holding the
    /// source's own enumerable keys minus the ones the pattern already
    /// named. The omit set rides in the spread sentinel's name;
    /// ObjectLit checking / lowering skip the listed keys.
    ///
    /// Shared by both pattern readers — the `PatShape` walker above
    /// (let / const / for-of heads) and the param walker in
    /// `destr_helpers` — so the sentinel protocol has one owner.
    pub(super) fn emit_obj_rest_let(
        &mut self,
        src_name: &str,
        omit: &[&str],
        rest_name: &str,
        mutable: bool,
        is_var: bool,
    ) -> Stmt {
        let obj = self.emit_obj_rest_expr(src_name, omit);
        Stmt::LetDecl {
            mutable,
            name: rest_name.to_string(),
            type_ann: None,
            init: obj,
            is_var,
        }
    }
}
