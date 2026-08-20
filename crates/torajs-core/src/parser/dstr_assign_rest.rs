//! RFC 20260820-dstr-deferred-close — the suspendable half of the
//! destructuring-assignment expansion: the deferred-close wrap every
//! suspendable pattern rides, and the 刀 D deferred-REST shape
//! (`[x, ...t[yield]] = src`), split out of `dstr_assign.rs` at the
//! size cap. The parent answers "how does a pattern expand into
//! slots"; this file answers "what changes when the expansion can
//! suspend".

use super::*;

impl<'a> Parser<'a> {
    /// 刀 D — does this pattern take the deferred-rest shape? A rest
    /// element AND a yield somewhere in the pattern (`expr_yield_temps`
    /// sees the hoisted `__yx_` reads, so this is a pure AST question
    /// and agrees with whatever the per-slot recovery later consumes).
    pub(super) fn detect_deferred_rest(&self, target: ExprId) -> bool {
        let Expr::Array(elems) = self.ast.get_expr(target) else {
            return false;
        };
        let has_rest = elems
            .last()
            .map(|&e| matches!(self.ast.get_expr(e), Expr::Spread { .. }))
            .unwrap_or(false);
        has_rest && !super::yield_hoist_events::expr_yield_temps(&self.ast, target).is_empty()
    }

    /// 刀 D — §13.15.5.5 AssignmentRestElement, target-first: the
    /// rest TARGET's reference evaluates (its yield suspends) before
    /// the drain. The emitted order carries the close obligations by
    /// construction:
    ///
    /// 1. recovered yield stmts — the suspension itself;
    /// 2. the target's object / key hoist — a throw here leaves the
    ///    park slot intact, so the pattern's finally still closes
    ///    (lref abrupt → [[done]] false → IteratorClose);
    /// 3. take the park into a fresh temp and CLEAR the slot — from
    ///    here [[done]] is true: a throw out of the drain (a `next()`
    ///    that throws) must NOT re-close (the thrw-close-skip family);
    /// 4. `target = __torajs_dstr_drain_rest(taken, raw)` — the drain
    ///    runs in value position, after the (already-hoisted)
    ///    reference, before PutValue.
    pub(super) fn emit_dstr_rest_deferred(
        &mut self,
        target: ExprId,
        id: u32,
        out: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        let is_pattern = matches!(
            self.ast.get_expr(target),
            Expr::Array(_) | Expr::ObjectLit { .. }
        );
        let mut rebuilt = target;
        if !is_pattern {
            if let Some(recovered) = self.recover_yield_temps(target)? {
                out.extend(recovered);
            }
            rebuilt = self.hoist_rest_lref(target, out)?;
        }
        let id2 = self.mint_desugar_id();
        let rest_name = format!("__dstra_rest_{id2}");
        let it_read = self.ast.add_expr(Expr::Ident(format!("__dstra_it_{id}")));
        out.push(Stmt::LetDecl {
            mutable: false,
            name: rest_name.clone(),
            type_ann: Some("any".into()),
            init: it_read,
            is_var: false,
        });
        let it_w = self.ast.add_expr(Expr::Ident(format!("__dstra_it_{id}")));
        let undef = self.ast.add_expr(Expr::Ident("undefined".into()));
        let clear = self.ast.add_expr(Expr::Assign {
            target: it_w,
            value: undef,
        });
        out.push(Stmt::Expr(clear));
        let callee = self
            .ast
            .add_expr(Expr::Ident("__torajs_dstr_drain_rest".into()));
        let a0 = self.ast.add_expr(Expr::Ident(rest_name));
        let a1 = self.ast.add_expr(Expr::Ident(format!("__dstra_raw_{id}")));
        let drain = self.ast.add_expr(Expr::Call {
            callee,
            args: vec![a0, a1],
        });
        self.emit_dstr_assign_slot(rebuilt, drain, out)
    }

    /// Hoist a Member / Index rest target's object (and key) into
    /// temps so their evaluation — and any throw it raises — lands
    /// BEFORE the park slot clears, while the PutValue through the
    /// rebuilt reference stays after the drain. Anything else (an
    /// Ident, an invalid target the slot path rejects) passes through.
    fn hoist_rest_lref(&mut self, target: ExprId, out: &mut Vec<Stmt>) -> Result<ExprId, String> {
        match self.ast.get_expr(target).clone() {
            Expr::Member { obj, name } => {
                let t = self.hoist_lref_part(obj, out);
                Ok(self.ast.add_expr(Expr::Member { obj: t, name }))
            }
            Expr::Index { obj, index } => {
                let t = self.hoist_lref_part(obj, out);
                let k = self.hoist_lref_part(index, out);
                Ok(self.ast.add_expr(Expr::Index { obj: t, index: k }))
            }
            _ => Ok(target),
        }
    }

    fn hoist_lref_part(&mut self, part: ExprId, out: &mut Vec<Stmt>) -> ExprId {
        let id = self.mint_desugar_id();
        let name = format!("__dstra_tref_{id}");
        out.push(Stmt::LetDecl {
            mutable: false,
            name: name.clone(),
            type_ann: None,
            init: part,
            is_var: false,
        });
        self.ast.add_expr(Expr::Ident(name))
    }

    /// RFC 20260820-dstr-deferred-close — a pattern whose evaluation
    /// can suspend (a recovered yield in a default or a target) must
    /// NOT close its iterator at the walk: §13.15.5.3 step 5 closes
    /// when the pattern COMPLETES, and an abrupt completion through a
    /// suspension (`gen.return()`, a throw) still owes the close on
    /// its way out. The walk parks the still-open iterator in
    /// `__dstra_it_<id>` (declared here, BEFORE the source temp so
    /// the lowering can see it; `undefined` = drained/never-opened →
    /// close no-op), and the element statements ride the engine-
    /// canonical wrap:
    ///
    /// ```text
    /// try { <elements> }
    /// catch (e) { threw = true; err = e; }
    /// finally {
    ///   try { __torajs_dstr_close_pending(it); }
    ///   finally { if (threw) throw err; }   // §7.4.6 step 7
    /// }
    /// ```
    ///
    /// The inner finally re-throws the original error OVER anything
    /// the close raised — the original completion wins; on the normal
    /// path the close's own throw (step 8) and the step-9 non-Object
    /// TypeError propagate. A generator `.return()` routes through
    /// the finally region (D3b), which is what runs the close.
    pub(super) fn wrap_deferred_close(&mut self, id: u32, out: &mut Vec<Stmt>, elems: Vec<Stmt>) {
        let it_name = format!("__dstra_it_{id}");
        let threw_name = format!("__dstra_threw_{id}");
        let err_name = format!("__dstra_err_{id}");
        let undef = self.ast.add_expr(Expr::Ident("undefined".into()));
        out.insert(
            0,
            Stmt::LetDecl {
                mutable: true,
                name: it_name.clone(),
                type_ann: Some("any".into()),
                init: undef,
                is_var: false,
            },
        );
        let f = self.ast.add_expr(Expr::Bool(false));
        out.push(Stmt::LetDecl {
            mutable: true,
            name: threw_name.clone(),
            type_ann: Some("boolean".into()),
            init: f,
            is_var: false,
        });
        let undef2 = self.ast.add_expr(Expr::Ident("undefined".into()));
        out.push(Stmt::LetDecl {
            mutable: true,
            name: err_name.clone(),
            type_ann: Some("any".into()),
            init: undef2,
            is_var: false,
        });
        let e_param = format!("__dstra_e_{id}");
        let threw_w = self.ast.add_expr(Expr::Ident(threw_name.clone()));
        let t = self.ast.add_expr(Expr::Bool(true));
        let set_threw = self.ast.add_expr(Expr::Assign {
            target: threw_w,
            value: t,
        });
        let err_w = self.ast.add_expr(Expr::Ident(err_name.clone()));
        let e_read = self.ast.add_expr(Expr::Ident(e_param.clone()));
        let set_err = self.ast.add_expr(Expr::Assign {
            target: err_w,
            value: e_read,
        });
        let close_callee = self
            .ast
            .add_expr(Expr::Ident("__torajs_dstr_close_pending".into()));
        let it_read = self.ast.add_expr(Expr::Ident(it_name));
        let close_call = self.ast.add_expr(Expr::Call {
            callee: close_callee,
            args: vec![it_read],
        });
        let threw_r = self.ast.add_expr(Expr::Ident(threw_name));
        let err_r = self.ast.add_expr(Expr::Ident(err_name));
        let rethrow = Stmt::If {
            cond: threw_r,
            then_branch: Box::new(Stmt::Block(vec![Stmt::Throw(err_r)])),
            else_branch: None,
        };
        let finally = vec![Stmt::Try {
            body: vec![Stmt::Expr(close_call)],
            had_catch: false,
            catch_param: None,
            catch_type: None,
            catch_body: Vec::new(),
            finally_body: Some(vec![rethrow]),
        }];
        out.push(Stmt::Try {
            body: elems,
            had_catch: true,
            catch_param: Some(e_param),
            catch_type: Some("any".into()),
            catch_body: vec![Stmt::Expr(set_threw), Stmt::Expr(set_err)],
            finally_body: Some(finally),
        });
    }
}
