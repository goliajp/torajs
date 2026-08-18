//! S2.24 (RFC 20260727-dstr-assignment 刀 1) — destructuring
//! assignment to existing bindings, ES §13.15.5.
//!
//! `[a, b] = src;` / `({ x: o.x } = src);` parse naturally into
//! `Expr::Assign { target: Array | ObjectLit, value }` — the LHS is
//! read as a literal first (the spec's CoverAssignmentPattern). The
//! statement path re-reads that literal as an AssignmentPattern and
//! expands it, parse-time, into a `Stmt::Multi` of plain assignments
//! (the same desugar family as let/const destructuring in
//! destr_drivers), so check / ssa_lower only ever see simple targets.
//!
//! The source ALWAYS hoists into a fresh `__dstra_src_N` temp — no
//! declaration-form ident-reuse: `[c, d] = c` under reuse would let
//! `c = c[0]` poison the later `c[1]` read, and the swap idiom
//! `[a, b] = [b, a]` depends on the RHS materializing first.
//!
//! Element grammar (array): Ident / Member / Index targets, `= D`
//! defaults (mirroring destr_defaults' length-guard ternary), holes,
//! trailing `...rest` (slice tail, same as the declaration form),
//! nested patterns (fresh temp + recursion). Object: shorthand /
//! renamed / Member-target fields, `f: y = D` defaults, nested
//! patterns, plus the §13.15.5 RequireObjectCoercible guard.
//!
//! Recorded boundaries (loud, never silent): object rest
//! (`{ ...r } = o` — remainder-copy semantics, not spread) rejects
//! here; `{ x = D }` shorthand defaults stay an upstream ObjectLit
//! parse error (CoverInitializedName); a pattern in a general
//! expression position (call arg, ternary arm) keeps check's
//! `invalid assignment target` — the statement-position CHAIN
//! (`result = [a, b] = vals`) is handled by
//! `try_desugar_assign_chain`.

use super::*;

impl<'a> Parser<'a> {
    /// Statement finisher: a statement-position `Expr::Assign` whose
    /// target parsed as an array / object literal is a destructuring
    /// assignment — expand it; anything else stays `Stmt::Expr`.
    pub(super) fn expr_stmt_or_dstr_assign(&mut self, expr: ExprId) -> Result<Stmt, String> {
        if let Some(stmts) = self.try_desugar_assign_chain(expr)? {
            return Ok(Stmt::Multi(stmts));
        }
        if let Expr::Assign { target, value } = self.ast.get_expr(expr)
            && matches!(
                self.ast.get_expr(*target),
                Expr::Array(_) | Expr::ObjectLit { .. }
            )
        {
            let (t, v) = (*target, *value);
            return Ok(Stmt::Multi(self.desugar_dstr_assign(t, v)?));
        }
        Ok(Stmt::Expr(expr))
    }

    /// Chained assignment through a pattern link, statement position —
    /// `result = [a, b] = vals` (the test262 dstr result-capture
    /// idiom, §13.15.2: the value of a destructuring assignment is
    /// the RHS reference itself, so `result` receives `vals`).
    ///
    /// Walk the `Expr::Assign` spine collecting links; rewrite only
    /// when ≥2 links and at least one is a pattern (a single pattern
    /// keeps the plain path below; a pattern-free chain is an
    /// ordinary nested assign expression and stays `Stmt::Expr`).
    /// The ultimate source hoists ONCE into `__dstra_chain_N`; every
    /// link then reads that temp right-to-left — a pattern link
    /// re-expands through [`Self::desugar_dstr_assign`], an ident
    /// link becomes a plain assignment. Reading the temp per link is
    /// pure, so the single-eval contract on the RHS holds.
    ///
    /// Recorded boundary (loud, never silent): a Member / Index link
    /// keeps check's `invalid assignment target` — its object
    /// expression would evaluate AFTER the RHS under this rewrite,
    /// and §13.15.2 orders it before.
    fn try_desugar_assign_chain(&mut self, expr: ExprId) -> Result<Option<Vec<Stmt>>, String> {
        let mut links: Vec<ExprId> = Vec::new();
        let mut saw_pattern = false;
        let mut cur = expr;
        while let Expr::Assign { target, value } = self.ast.get_expr(cur) {
            let (t, v) = (*target, *value);
            match self.ast.get_expr(t) {
                Expr::Array(_) | Expr::ObjectLit { .. } => saw_pattern = true,
                Expr::Ident(_) => {}
                _ => return Ok(None),
            }
            links.push(t);
            cur = v;
        }
        if !saw_pattern || links.len() < 2 {
            return Ok(None);
        }
        let id = self.mint_desugar_id();
        let tname = format!("__dstra_chain_{id}");
        let mut out = vec![Stmt::LetDecl {
            mutable: false,
            name: tname.clone(),
            type_ann: None,
            init: cur,
            is_var: false,
        }];
        for t in links.iter().rev() {
            let tref = self.ast.add_expr(Expr::Ident(tname.clone()));
            if matches!(
                self.ast.get_expr(*t),
                Expr::Array(_) | Expr::ObjectLit { .. }
            ) {
                out.extend(self.desugar_dstr_assign(*t, tref)?);
            } else {
                let assign = self.ast.add_expr(Expr::Assign {
                    target: *t,
                    value: tref,
                });
                out.push(Stmt::Expr(assign));
            }
        }
        Ok(Some(out))
    }

    /// Pattern-assignment expansion entry, shared with the for-of
    /// bare-pattern head (刀 2): hoist the source into a fresh temp,
    /// then walk the pattern emitting one plain assignment per slot.
    pub(super) fn desugar_dstr_assign(
        &mut self,
        target: ExprId,
        value: ExprId,
    ) -> Result<Vec<Stmt>, String> {
        let id = self.mint_desugar_id();
        let src_name = format!("__dstra_src_{id}");
        self.note_ary_destr_group(target, value);
        let mut out = vec![Stmt::LetDecl {
            mutable: false,
            name: src_name.clone(),
            type_ann: None,
            init: value,
            is_var: false,
        }];
        self.emit_dstr_assign_pattern(target, &src_name, &mut out)?;
        Ok(out)
    }

    /// Register the temp's init in `ary_destr_groups`, exactly as the
    /// declaration form does (destr_drivers / destr_helpers): the
    /// group entry is what routes a non-indexable source through the
    /// iterator lane AND what keeps a short source's past-end slots
    /// reading `undefined` instead of the typed lane's OOB behavior.
    fn note_ary_destr_group(&mut self, pat: ExprId, src_expr: ExprId) {
        if let Expr::Array(elems) = self.ast.get_expr(pat) {
            let has_rest = elems
                .last()
                .map(|&e| matches!(self.ast.get_expr(e), Expr::Spread { .. }))
                .unwrap_or(false);
            let limit = if has_rest { -1 } else { elems.len() as i64 };
            self.ast.ary_destr_groups.insert(src_expr, limit);
        }
    }

    fn emit_dstr_assign_pattern(
        &mut self,
        pat: ExprId,
        src_name: &str,
        out: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        match self.ast.get_expr(pat).clone() {
            Expr::Array(elems) => {
                // `[...x,] = src` — the literal parsed fine, but the
                // pattern re-read requires the rest element to be
                // LAST; the trailing comma broke that (§13.15.1).
                if self.ast.arrlit_trailing_comma_after_rest.contains(&pat) {
                    return Err(format!(
                        "rest element must be last in a destructuring pattern at {}",
                        self.at()
                    ));
                }
                self.emit_dstr_assign_array(&elems, src_name, out)
            }
            Expr::ObjectLit { fields } => self.emit_dstr_assign_object(&fields, src_name, out),
            _ => Err(format!(
                "invalid destructuring assignment target at {}",
                self.at()
            )),
        }
    }

    /// §13.15.5.5 IteratorDestructuringAssignmentEvaluation over the
    /// index-read approximation the declaration form already uses.
    fn emit_dstr_assign_array(
        &mut self,
        elems: &[ExprId],
        src_name: &str,
        out: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        for (i, &el) in elems.iter().enumerate() {
            match self.ast.get_expr(el).clone() {
                // Elision advances the position, binds nothing.
                Expr::Elision => {}
                Expr::Spread { expr } => {
                    if i + 1 != elems.len() {
                        return Err(format!(
                            "rest element must be last in a destructuring pattern at {}",
                            self.at()
                        ));
                    }
                    let src_ref = self.ast.add_expr(Expr::Ident(src_name.to_string()));
                    let slice_m = self.ast.add_expr(Expr::Member {
                        obj: src_ref,
                        name: "slice".into(),
                    });
                    let start = self.ast.add_expr(Expr::Number(i as f64));
                    let tail = self.ast.add_expr(Expr::Call {
                        callee: slice_m,
                        args: vec![start],
                    });
                    self.emit_dstr_assign_slot(expr, tail, out)?;
                }
                // `[v = 10]` — the element parsed as an Assign; its
                // value is the §13.15.5.3 default.
                Expr::Assign {
                    target,
                    value: default,
                } => {
                    if super::yield_expr_hoist::expr_reads_yield_temp(&self.ast, default) {
                        return Err(format!(
                            "not yet supported: `yield` in a destructuring-assignment \
                             default (conditional position) at {}",
                            self.at()
                        ));
                    }
                    // §8.4.5 NamedEvaluation reaches assignment-pattern
                    // defaults too — and the registry entry is ALSO the
                    // hoisted-generator wrap axis's key (`[g =
                    // function*(){}] = []` panicked at box_to_any
                    // without it; the binding lane records upstream).
                    if let Expr::Ident(b) = self.ast.get_expr(target) {
                        let b = b.clone();
                        self.record_dstr_default_name(default, &b);
                    }
                    let load = self.dstra_elem_load(src_name, i, Some(default));
                    self.emit_dstr_assign_slot(target, load, out)?;
                }
                _ => {
                    let load = self.dstra_elem_load(src_name, i, None);
                    self.emit_dstr_assign_slot(el, load, out)?;
                }
            }
        }
        Ok(())
    }

    /// §13.15.5.4 keyed destructuring, plus the RequireObjectCoercible
    /// guard the declaration form emits (null / undefined source is a
    /// TypeError even for `{}`).
    fn emit_dstr_assign_object(
        &mut self,
        fields: &[(String, ExprId)],
        src_name: &str,
        out: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        out.push(self.emit_object_coercible_guard(src_name));
        for (i, (fname, val)) in fields.iter().enumerate() {
            // §13.15.5.4 — AssignmentRestProperty takes every own
            // enumerable key the earlier fields did not name. Same
            // construction the declaration form uses; the only thing
            // that differs is where it lands, so it goes through the
            // ordinary target slot (an Ident, a member, an index)
            // rather than binding a fresh name.
            if fname == "__spread__" {
                if i + 1 != fields.len() {
                    return Err(format!(
                        "rest property must be last in a destructuring pattern at {}",
                        self.at()
                    ));
                }
                // Recorded boundary (loud, never silent): the omit
                // set carries static names only — a computed key's
                // `__computed_N__` sentinel in it would omit the
                // wrong property, so the coexistence rejects (same
                // rule as the declaration lanes).
                if fields[..i]
                    .iter()
                    .any(|(n, _)| n.starts_with("__computed_"))
                {
                    return Err(format!(
                        "not yet supported: object rest alongside a computed key \
                         in the same destructuring pattern at {}",
                        self.at()
                    ));
                }
                let omit: Vec<&str> = fields[..i].iter().map(|(n, _)| n.as_str()).collect();
                let rest_obj = self.emit_obj_rest_expr(src_name, &omit);
                self.emit_dstr_assign_slot(*val, rest_obj, out)?;
                continue;
            }
            // §13.15.5.4 with a ComputedPropertyName — the objlit
            // cover parse folded `[expr]:` into a `__computed_N__`
            // sentinel plus the `objlit_computed_keys` side table
            // (value ExprId → key expr); un-fold it here instead of
            // member-reading the sentinel name. The key hoists into a
            // `__ck_N` temp in field order, the load is the shared
            // any-key recipe.
            if let Some(&key_expr) = self.ast.objlit_computed_keys.get(val) {
                let id = self.mint_desugar_id();
                let kname = format!("__ck_{id}");
                out.push(Stmt::LetDecl {
                    mutable: false,
                    name: kname.clone(),
                    type_ann: None,
                    init: key_expr,
                    is_var: false,
                });
                let (target, default) = match self.ast.get_expr(*val) {
                    Expr::Assign { target, value } => (*target, Some(*value)),
                    _ => (*val, None),
                };
                if let Some(d) = default
                    && super::yield_expr_hoist::expr_reads_yield_temp(&self.ast, d)
                {
                    return Err(format!(
                        "not yet supported: `yield` in a destructuring-assignment \
                         default (conditional position) at {}",
                        self.at()
                    ));
                }
                if let Some(d) = default
                    && let Expr::Ident(b) = self.ast.get_expr(target)
                {
                    let b = b.clone();
                    self.record_dstr_default_name(d, &b);
                }
                let load = self.dstra_computed_load(src_name, &kname, default);
                self.emit_dstr_assign_slot(target, load, out)?;
                continue;
            }
            let (target, default) = match self.ast.get_expr(*val) {
                // `{ f: y = D }` — the field value parsed as an
                // Assign; its value is the default.
                Expr::Assign { target, value } => (*target, Some(*value)),
                _ => (*val, None),
            };
            if let Some(d) = default
                && super::yield_expr_hoist::expr_reads_yield_temp(&self.ast, d)
            {
                return Err(format!(
                    "not yet supported: `yield` in a destructuring-assignment \
                     default (conditional position) at {}",
                    self.at()
                ));
            }
            // §8.4.5 NamedEvaluation for the field default (also the
            // hoisted-generator wrap axis's key — see the array lane).
            if let Some(d) = default
                && let Expr::Ident(b) = self.ast.get_expr(target)
            {
                let b = b.clone();
                self.record_dstr_default_name(d, &b);
            }
            let load = self.dstra_field_load(src_name, fname, default);
            self.emit_dstr_assign_slot(target, load, out)?;
        }
        Ok(())
    }

    /// One pattern slot: a simple target gets a direct assign; a
    /// nested pattern hoists the loaded value into a fresh temp and
    /// recurses; anything else is the spec's early error.
    fn emit_dstr_assign_slot(
        &mut self,
        target: ExprId,
        loaded: ExprId,
        out: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        // `0, { yield } = {}` — the shorthand hoisted to a `__yx_`
        // temp, which is not a valid assignment target (§13.15.1).
        self.reject_invalid_assignment_target(target)?;
        // §13.15.1 — `eval` / `arguments` are not valid simple
        // assignment targets in strict code (module code always is),
        // and `yield` is a strict-mode reserved word outright
        // (rotation 346 — `0, { yield } = {}` must reject at parse
        // phase; the checker-side admits landed this rotation let it
        // fall through to a runtime unknown-ident instead).
        if let Expr::Ident(n) = self.ast.get_expr(target)
            && (n == "arguments" || n == "eval" || n == "yield")
        {
            return Err(format!(
                "`{n}` is not a valid assignment target in a destructuring pattern at {} \
                 (ES §13.15.1)",
                self.at()
            ));
        }
        let is_simple = matches!(
            self.ast.get_expr(target),
            Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
        );
        if is_simple {
            let assign = self.ast.add_expr(Expr::Assign {
                target,
                value: loaded,
            });
            out.push(Stmt::Expr(assign));
            return Ok(());
        }
        if matches!(
            self.ast.get_expr(target),
            Expr::Array(_) | Expr::ObjectLit { .. }
        ) {
            let id = self.mint_desugar_id();
            let tmp = format!("__dstra_src_{id}");
            self.note_ary_destr_group(target, loaded);
            out.push(Stmt::LetDecl {
                mutable: false,
                name: tmp.clone(),
                type_ann: None,
                init: loaded,
                is_var: false,
            });
            return self.emit_dstr_assign_pattern(target, &tmp, out);
        }
        Err(format!(
            "invalid destructuring assignment target at {}",
            self.at()
        ))
    }
}
