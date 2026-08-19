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
//! - The shared load recipes (`dstra_elem_load` / `dstra_field_load` /
//!   `dstra_computed_load`) live HERE — array defaults carry the
//!   length-guard, object defaults the undefined-gate — so §13.3.3
//!   default semantics cannot drift between the declaration, param
//!   and assignment lanes (all three read them).

use super::destr_shape::{AryElem, FieldKey, ObjBinding, ObjField, PatShape, RestShape};
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
        for ObjField { key, binding } in fields {
            match binding {
                ObjBinding::Bind { name, default } => {
                    if let Some(d) = default {
                        self.record_dstr_default_name(*d, name);
                    }
                    let init = self.dstra_key_load(&src_name, key, *default, out);
                    out.push(Stmt::LetDecl {
                        mutable,
                        name: name.clone(),
                        type_ann: None,
                        init,
                        is_var,
                    });
                }
                ObjBinding::Nested { pat, default } => {
                    let loaded = self.dstra_key_load(&src_name, key, *default, out);
                    self.emit_pattern_binds(pat, loaded, mutable, is_var, out);
                }
            }
        }
        if let Some(rest_name) = rest {
            // Named keys only — the reader rejects a computed key
            // alongside rest (recorded boundary), so no key is lost.
            let omit: Vec<&str> = fields
                .iter()
                .filter_map(|f| match &f.key {
                    FieldKey::Named(n) => Some(n.as_str()),
                    FieldKey::Computed(_) => None,
                })
                .collect();
            let bind = self.emit_obj_rest_let(&src_name, &omit, rest_name, mutable, is_var);
            out.push(bind);
        }
    }

    /// One field's load, key-kind dispatched. A named key reads
    /// through the shared `dstra_field_load` recipe; a computed key
    /// first hoists its expression into a `__ck_N` temp (pushed onto
    /// `out`, so §14.3.3.3 order holds: the key evaluates after the
    /// previous field's bind and before this field's load), then reads
    /// through `dstra_computed_load`.
    fn dstra_key_load(
        &mut self,
        src_name: &str,
        key: &FieldKey,
        default: Option<ExprId>,
        out: &mut Vec<Stmt>,
    ) -> ExprId {
        match key {
            FieldKey::Named(field) => self.dstra_field_load(src_name, field, default),
            FieldKey::Computed(key_expr) => {
                let id = self.mint_desugar_id();
                let kname = format!("__ck_{id}");
                out.push(Stmt::LetDecl {
                    mutable: false,
                    name: kname.clone(),
                    type_ann: None,
                    init: *key_expr,
                    is_var: false,
                });
                self.dstra_computed_load(src_name, &kname, default)
            }
        }
    }

    /// `__t[__ck]`, optionally wrapped in the §13.15.5.4 default
    /// ternary (undefined and ONLY undefined fires the default). The
    /// key temp is a plain ident read, so the double reference in the
    /// ternary re-reads the property — the same double-GetV the static
    /// field recipe carries. The any-key index lane answers
    /// `undefined` for an absent key, so no lenient mark is needed.
    /// `pub(super)`: shared with the param walker (destr_helpers) and
    /// the assignment lane (dstr_assign).
    pub(super) fn dstra_computed_load(
        &mut self,
        src_name: &str,
        key_name: &str,
        default: Option<ExprId>,
    ) -> ExprId {
        let src_ref = self.ast.add_expr(Expr::Ident(src_name.to_string()));
        let key_ref = self.ast.add_expr(Expr::Ident(key_name.to_string()));
        let load = self.ast.add_expr(Expr::Index {
            obj: src_ref,
            index: key_ref,
        });
        let Some(default_expr) = default else {
            return load;
        };
        let undef = self.ast.add_expr(Expr::Ident("undefined".into()));
        let cond = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left: load,
            right: undef,
        });
        self.ast.add_expr(Expr::Ternary {
            cond,
            then_branch: default_expr,
            else_branch: load,
        })
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
        // The rest object is extensible — record the binding so a
        // member read missing its static anchor rides the runtime
        // [[Get]] probe instead of the typo reject (consumers:
        // `answer_terminal_miss` / `ssa_lower_member`).
        self.ast.obj_rest_names.insert(rest_name.to_string());
        Stmt::LetDecl {
            mutable,
            name: rest_name.to_string(),
            type_ann: None,
            init: obj,
            is_var,
        }
    }

    /// `__t[i]`, optionally wrapped in the §13.15.5.3 default ternary
    /// (mirrors maybe_parse_destr_default: fires past-end — the
    /// length guard also keeps typed-lane OOB reads out — and on an
    /// explicit undefined element; null / 0 / '' keep their value).
    /// `pub(super)`: shared with the declaration-position PatShape
    /// emitter (destr_shape.rs) so both lanes read one recipe.
    pub(super) fn dstra_elem_load(
        &mut self,
        src_name: &str,
        idx: usize,
        default: Option<ExprId>,
    ) -> ExprId {
        let src_ref = self.ast.add_expr(Expr::Ident(src_name.to_string()));
        let idx_e = self.ast.add_expr(Expr::Number(idx as f64));
        let load = self.ast.add_expr(Expr::Index {
            obj: src_ref,
            index: idx_e,
        });
        let Some(default_expr) = default else {
            return load;
        };
        let src_ref2 = self.ast.add_expr(Expr::Ident(src_name.to_string()));
        let len_member = self.ast.add_expr(Expr::Member {
            obj: src_ref2,
            name: "length".into(),
        });
        let idx_lit = self.ast.add_expr(Expr::Number(idx as f64));
        let len_ok = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Gt,
            left: len_member,
            right: idx_lit,
        });
        let undef = self.ast.add_expr(Expr::Ident("undefined".into()));
        let not_undef = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Neq,
            left: load,
            right: undef,
        });
        let cond = self.ast.add_expr(Expr::BinOp {
            op: BinOp::LAnd,
            left: len_ok,
            right: not_undef,
        });
        self.ast.add_expr(Expr::Ternary {
            cond,
            then_branch: load,
            else_branch: default_expr,
        })
    }

    /// `__t.f`, optionally wrapped in the §13.15.5.4 default ternary
    /// (mirrors maybe_parse_object_destr_default: undefined and ONLY
    /// undefined fires the default). `pub(super)`: shared with
    /// destr_shape.rs, same as dstra_elem_load.
    pub(super) fn dstra_field_load(
        &mut self,
        src_name: &str,
        field: &str,
        default: Option<ExprId>,
    ) -> ExprId {
        // §13.3.3 PropertyName : NumericLiteral — an all-digit field
        // (`{ 0: v }`) is an index read, not a member read (`src.0`
        // is not a member the lowering can express; `src[0]` is the
        // canonical access for arrays-as-objects and dynobjs alike).
        // The elem-load recipe carries the length-guard, so a
        // past-end key answers undefined (and fires the default)
        // instead of the typed lane's OOB RangeError.
        if !field.is_empty() && field.bytes().all(|b| b.is_ascii_digit()) {
            let idx = field.parse::<usize>().unwrap_or(0);
            return self.dstra_elem_load(src_name, idx, default);
        }
        let src_ref = self.ast.add_expr(Expr::Ident(src_name.to_string()));
        let load = self.ast.add_expr(Expr::Member {
            obj: src_ref,
            name: field.to_string(),
        });
        // §13.15.5.4 GetV answers `undefined` for an absent field on
        // EVERY pattern slot — a default only changes what replaces
        // that `undefined`, not whether the read is allowed. So the
        // lenient mark goes on every load this recipe mints, not just
        // the default-guarded ones; see `Ast::dstr_default_member_loads`.
        self.ast.dstr_default_member_loads.insert(load);
        let Some(default_expr) = default else {
            return load;
        };
        let undef = self.ast.add_expr(Expr::Ident("undefined".into()));
        let cond = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left: load,
            right: undef,
        });
        self.ast.add_expr(Expr::Ternary {
            cond,
            then_branch: default_expr,
            else_branch: load,
        })
    }

    /// §13.15.5.3/4 with a `yield` in the Initializer — the spec
    /// evaluates the default only when the slot answered undefined,
    /// so its yield is CONDITIONAL, and the expression-level ternary
    /// the plain recipes mint cannot carry it (the hoist already
    /// moved the `YieldInto` in front of the whole statement). This
    /// lane re-emits the recovered `YieldInto` stmts inside a
    /// statement-level undefined-guard instead:
    ///
    /// ```text
    /// let __dv = <plain load>;
    /// if (__dv === undefined) { <YieldInto ...>; __dv = <default>; }
    /// <target> = __dv;
    /// ```
    ///
    /// `plain_load` must already answer undefined for an absent slot
    /// (the callers pass the undefined-defaulted elem recipe or the
    /// bare field/computed load).
    pub(super) fn emit_conditional_default(
        &mut self,
        target: ExprId,
        plain_load: ExprId,
        default: ExprId,
        recovered: Vec<Stmt>,
        out: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        let id = self.mint_desugar_id();
        let dv = format!("__dv_{id}");
        out.push(Stmt::LetDecl {
            mutable: true,
            name: dv.clone(),
            type_ann: Some("any".into()),
            init: plain_load,
            is_var: false,
        });
        let dv_read = self.ast.add_expr(Expr::Ident(dv.clone()));
        let undef = self.ast.add_expr(Expr::Ident("undefined".into()));
        let cond = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left: dv_read,
            right: undef,
        });
        let mut then = recovered;
        let dv_write = self.ast.add_expr(Expr::Ident(dv.clone()));
        let assign = self.ast.add_expr(Expr::Assign {
            target: dv_write,
            value: default,
        });
        then.push(Stmt::Expr(assign));
        out.push(Stmt::If {
            cond,
            then_branch: Box::new(Stmt::Block(then)),
            else_branch: None,
        });
        let dv_val = self.ast.add_expr(Expr::Ident(dv));
        self.emit_dstr_assign_slot(target, dv_val, out)
    }
}
