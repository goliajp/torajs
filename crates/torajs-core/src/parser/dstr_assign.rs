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
use crate::ast::PropKey;

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
        // 刀 D — a rest element in a SUSPENDABLE pattern takes the
        // deferred shape: the raw source hoists (the drain needs it
        // alive after the suspension), the walk limit is the bounded
        // prefix, and the group is flagged so the checker keeps it on
        // the iterator lane even for a statically indexable source.
        let rest_susp = self.detect_deferred_rest(target);
        let group_init = if rest_susp {
            let raw_name = format!("__dstra_raw_{id}");
            self.dstra_deferred_rest_ids.insert(id);
            Some(raw_name)
        } else {
            None
        };
        let mut out = Vec::new();
        let src_init = match &group_init {
            Some(raw_name) => {
                // Both temps pin `any` (unlike the plain path below):
                // the checker forces this group onto the iterator lane
                // regardless of the source's static type, and the
                // generator lift's sniff would otherwise answer the
                // RAW alias's typed lane for the src field — which the
                // lowering's field-walk gate (field_ty == Any) then
                // refuses, leaving the park slot empty. The raw temp
                // is also the drain kernel's `recv` argument, which
                // takes an AnyValue.
                out.push(Stmt::LetDecl {
                    mutable: false,
                    name: raw_name.clone(),
                    type_ann: Some("any".into()),
                    init: value,
                    is_var: false,
                });
                let raw_ref = self.ast.add_expr(Expr::Ident(raw_name.clone()));
                if let Expr::Array(elems) = self.ast.get_expr(target) {
                    let prefix = (elems.len() - 1) as i64;
                    self.ast.ary_destr_groups.insert(raw_ref, prefix);
                    self.ast.dstr_deferred_rest.insert(raw_ref);
                }
                raw_ref
            }
            None => {
                self.note_ary_destr_group(target, value);
                value
            }
        };
        // No annotation here even inside a generator: when a
        // slot-position yield puts this temp across a suspension
        // point, the state-machine lift asks the field-annotation
        // sniff first (an ArrayLit source keeps its typed lane), and
        // for `__dstra_src_*` its FALLBACK is `any` instead of
        // `number` — see desugar_generators_walkers. Pinning `any`
        // here would downgrade sniffable sources onto the any lane.
        // EXCEPT the deferred-rest shape, which pins `any` on purpose
        // (see the raw hoist above).
        out.push(Stmt::LetDecl {
            mutable: false,
            name: src_name.clone(),
            type_ann: rest_susp.then(|| "any".into()),
            init: src_init,
            is_var: false,
        });
        let saved = std::mem::replace(&mut self.dstra_saw_yield, false);
        let mut elems = Vec::new();
        self.emit_dstr_assign_pattern(target, &src_name, &mut elems)?;
        let suspends = std::mem::replace(&mut self.dstra_saw_yield, saved);
        if suspends || rest_susp {
            self.wrap_deferred_close(id, &mut out, elems);
        } else {
            out.extend(elems);
        }
        Ok(out)
    }

    /// Register the temp's init in `ary_destr_groups`, exactly as the
    /// declaration form does (destr_drivers / destr_helpers): the
    /// group entry is what routes a non-indexable source through the
    /// iterator lane AND what keeps a short source's past-end slots
    /// reading `undefined` instead of the typed lane's OOB behavior.
    pub(super) fn note_ary_destr_group(&mut self, pat: ExprId, src_expr: ExprId) {
        if let Expr::Array(elems) = self.ast.get_expr(pat) {
            let has_rest = elems
                .last()
                .map(|&e| matches!(self.ast.get_expr(e), Expr::Spread { .. }))
                .unwrap_or(false);
            let limit = if has_rest { -1 } else { elems.len() as i64 };
            self.ast.ary_destr_groups.insert(src_expr, limit);
        }
    }

    pub(super) fn emit_dstr_assign_pattern(
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
                    // 刀 D — a deferred-rest pattern (flagged by id at
                    // the top of the expansion) drains from the park
                    // instead of slicing the bounded prefix array.
                    if let Some(id) = src_name
                        .strip_prefix("__dstra_src_")
                        .and_then(|s| s.parse::<u32>().ok())
                        && self.dstra_deferred_rest_ids.contains(&id)
                    {
                        self.emit_dstr_rest_deferred(expr, id, out)?;
                        continue;
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
                    // §8.4.5 NamedEvaluation reaches assignment-pattern
                    // defaults too — and the registry entry is ALSO the
                    // hoisted-generator wrap axis's key (`[g =
                    // function*(){}] = []` panicked at box_to_any
                    // without it; the binding lane records upstream).
                    if let Expr::Ident(b) = self.ast.get_expr(target) {
                        let b = b.clone();
                        self.record_dstr_default_name(default, &b);
                    }
                    if let Some(recovered) = self.recover_yield_temps(default)? {
                        // A yield in the default — the recipe's
                        // literal-undefined default keeps the OOB /
                        // hole answer while the real default (and its
                        // yield) moves under the statement guard.
                        let undef = self.ast.add_expr(Expr::Ident("undefined".into()));
                        let plain = self.dstra_elem_load(src_name, i, Some(undef));
                        self.emit_conditional_default(target, plain, default, recovered, out)?;
                    } else {
                        let load = self.dstra_elem_load(src_name, i, Some(default));
                        self.emit_dstr_assign_slot(target, load, out)?;
                    }
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
        fields: &[(PropKey, ExprId)],
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
                // The omit list is identifier-spelled (`__spread_omit__:`
                // sentinel); a lone-surrogate sibling key is not a name
                // the rest object could exclude by that spelling.
                let omit: Vec<&str> = fields[..i].iter().filter_map(|(n, _)| n.as_str()).collect();
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
                    && let Expr::Ident(b) = self.ast.get_expr(target)
                {
                    let b = b.clone();
                    self.record_dstr_default_name(d, &b);
                }
                if let Some(d) = default
                    && let Some(recovered) = self.recover_yield_temps(d)?
                {
                    let plain = self.dstra_computed_load(src_name, &kname, None);
                    self.emit_conditional_default(target, plain, d, recovered, out)?;
                    continue;
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
            // §8.4.5 NamedEvaluation for the field default (also the
            // hoisted-generator wrap axis's key — see the array lane).
            if let Some(d) = default
                && let Expr::Ident(b) = self.ast.get_expr(target)
            {
                let b = b.clone();
                self.record_dstr_default_name(d, &b);
            }
            if let Some(d) = default
                && let Some(recovered) = self.recover_yield_temps(d)?
            {
                let plain = self.dstra_field_load(src_name, fname, None);
                self.emit_conditional_default(target, plain, d, recovered, out)?;
                continue;
            }
            let load = self.dstra_field_load(src_name, fname, default);
            self.emit_dstr_assign_slot(target, load, out)?;
        }
        Ok(())
    }
}
