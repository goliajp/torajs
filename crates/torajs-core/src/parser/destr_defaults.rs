//! ES §13.15.5.3 / §13.15.5.4 BindingElement initializers — the `= D`
//! tail of a destructuring slot (chunk B of RFC 20260714-dstr-residual
//! blade 3, split out of `destr_helpers.rs` at the 500-line limit).
//!
//! Two shapes, one rule: the default fires when the destructured value
//! is `undefined` and ONLY then (an explicit `null` / `0` / `''` keeps
//! its value). The array form additionally guards on the source's
//! length, which keeps a typed-lane out-of-bounds read from ever
//! happening.
//!
//! Both are `pub(super)`, called from `destr_helpers` and
//! `destr_drivers`.

use super::*;

impl<'a> Parser<'a> {
    /// P-PARSE.3 — peek for a `=` after a destr slot binding and
    /// wrap the load expression in a length-check ternary that
    /// substitutes the default when the source iterator is
    /// "exhausted" at this index (src.length <= elem_idx). The
    /// spec also fires the default when the value is `undefined`,
    /// but tora has no real undefined yet (P1) — once that lands
    /// the ternary should also test `=== undefined`.
    pub(super) fn maybe_parse_destr_default(
        &mut self,
        load_expr: ExprId,
        src_name: String,
        elem_idx: usize,
        binding: Option<&str>,
    ) -> Result<ExprId, String> {
        if !matches!(self.peek(), Token::Eq) {
            return Ok(load_expr);
        }
        self.pos += 1; // consume `=`
        // Default initializers evaluate conditionally (only on
        // undefined) — no yield hoist.
        let default_expr = self.with_yield_hoist_disallowed(|p| p.parse_expr())?;
        if let Some(b) = binding {
            self.record_dstr_default_name(default_expr, b);
        }
        // Build: src.length > elem_idx && src[i] !== undefined
        //          ? load_expr : default_expr
        // Per ES §13.15.5.3 IteratorBindingInitialization the default
        // fires when the iterator value IS undefined — both past-end
        // (length check, which also keeps typed-lane OOB reads out)
        // and an explicit undefined element (`[a = 5] = [undefined]`).
        let src_ref = self.ast.add_expr(Expr::Ident(src_name));
        let len_member = self.ast.add_expr(Expr::Member {
            obj: src_ref,
            name: "length".into(),
        });
        let idx_lit = self.ast.add_expr(Expr::Number(elem_idx as f64));
        let len_ok = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Gt,
            left: len_member,
            right: idx_lit,
        });
        let undef_ident = self.ast.add_expr(Expr::Ident("undefined".into()));
        let not_undef = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Neq,
            left: load_expr,
            right: undef_ident,
        });
        let cond = self.ast.add_expr(Expr::BinOp {
            op: BinOp::LAnd,
            left: len_ok,
            right: not_undef,
        });
        Ok(self.ast.add_expr(Expr::Ternary {
            cond,
            then_branch: load_expr,
            else_branch: default_expr,
        }))
    }

    /// P-PARSE.3 — `{ x = D }` / `{ x: y = D }`. Per ES spec
    /// §13.15.5.4 KeyedDestructuringAssignmentEvaluation the
    /// default fires when the looked-up value is `undefined` —
    /// and ONLY undefined: an explicit `null` field keeps null
    /// (test262 obj-ptrn-id-init-skipped asserts the initializer
    /// never evaluates for null/0/false/''). The pre-undefined-era
    /// form compared `=== null`, which both missed any-lane absent
    /// fields (undefined) and wrongly fired on explicit null.
    pub(super) fn maybe_parse_object_destr_default(
        &mut self,
        load_expr: ExprId,
        binding: Option<&str>,
    ) -> Result<ExprId, String> {
        if !matches!(self.peek(), Token::Eq) {
            return Ok(load_expr);
        }
        self.pos += 1; // consume `=`
        // Default initializers evaluate conditionally (only on
        // undefined) — no yield hoist.
        let default_expr = self.with_yield_hoist_disallowed(|p| p.parse_expr())?;
        if let Some(b) = binding {
            self.record_dstr_default_name(default_expr, b);
        }
        // load_expr === undefined ? default_expr : load_expr
        let undef_ident = self.ast.add_expr(Expr::Ident("undefined".into()));
        let cond = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left: load_expr,
            right: undef_ident,
        });
        Ok(self.ast.add_expr(Expr::Ternary {
            cond,
            then_branch: default_expr,
            else_branch: load_expr,
        }))
    }
}
