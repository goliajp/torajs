//! ②.6b (ann-width RFC §5.7) — built-in Promise chain lowering:
//! `.then` / `.catch` / `.finally` call sites, the f64-face callback
//! wrap (see `ssa_lower_promise_thunk.rs` for the synthesized
//! adapters), and the promise value-point width query the await /
//! resolve sites share. Extracted from `ssa_lower.rs` (file-size
//! known-debt).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{
    CLOSURE_CAP_BASE_OFF, CLOSURE_DROP_FN_OFF, CLOSURE_FN_ADDR_OFF, CLOSURE_PROPS_OFF,
    intern_fn_sig,
};
use crate::ssa_lower_promise_thunk::PTHUNK_ENV_SIZE;

impl crate::ssa_lower::LowerCtx<'_> {
    /// ②.6b — is the promise value point of `obj`'s expr f64? Mirrors
    /// the analysis-side container_key_lookup shallow shapes: an Ident
    /// queries its slot keys (Local/Global and, in a named fn, the
    /// Param spelling — only the one the analysis populated answers);
    /// a direct call of a known fn queries the fn's Ret key (the
    /// analysis keys `await presolved(x)` through `Ret(presolved)`,
    /// NOT an Anon — the original Anon spelling read narrow and the
    /// let-store sitofp'd raw f64 bits); anything else its Anon
    /// origin. The `value` projection matches the parser's
    /// `await p` → `p.value` desugar spelling.
    pub(crate) fn promise_value_is_f64(&self, obj: ExprId) -> bool {
        use crate::num_width::SlotKey;
        match self.ast.get_expr(obj) {
            Expr::Ident(n) => {
                self.num_f64_slots
                    .field_is_f64(&self.num_width_local_key(n), "value")
                    || (!self.is_main_fn
                        && self
                            .num_f64_slots
                            .field_is_f64(&SlotKey::Param(self.f.name.clone(), n.clone()), "value"))
            }
            Expr::Call { callee, .. } => {
                let fname = self.call_retargets.get(&obj).cloned().or_else(|| {
                    match self.ast.get_expr(*callee) {
                        Expr::Ident(n) if self.fn_table.contains_key(n) => Some(n.clone()),
                        _ => None,
                    }
                });
                let key = match fname {
                    Some(f) => SlotKey::Ret(f),
                    None => SlotKey::Anon(obj.0),
                };
                self.num_f64_slots.field_is_f64(&key, "value")
            }
            _ => self
                .num_f64_slots
                .field_is_f64(&SlotKey::Anon(obj.0), "value"),
        }
    }

    /// ②.6b — wrap an f64-faced `.then` / `.catch` handler in its
    /// synthesized bits-ABI adapter. The promise runtime invokes
    /// every handler as `(env, i64) -> i64` raw bits; a callback
    /// whose negotiated signature carries an F64 face would cross
    /// that boundary in the wrong register bank. The wrap is a
    /// closure-shaped env (fn slot → thunk, capture 0 → the real
    /// callback) so the runtime's closure dispatcher works unchanged.
    /// Integral-faced callbacks pass through untouched.
    pub(crate) fn maybe_wrap_promise_cb(&mut self, cb_op: Operand) -> Operand {
        let cb_ty = self.operand_ty(&cb_op);
        let (sid, is_closure) = match cb_ty {
            Type::Closure(s) => (s, true),
            Type::FnSig(s) => (s, false),
            _ => return cb_op,
        };
        let (params, ret) = self.fn_sigs[sid.0 as usize].clone();
        let p = params.first() == Some(&Type::F64);
        let r = ret == Type::F64;
        if !p && !r {
            return cb_op;
        }
        let Some((thunk_fid, thunk_sig)) = self.promise_thunks.get(is_closure, p, r) else {
            return cb_op;
        };
        let drop_pair = if is_closure {
            self.promise_thunks.drop_closure
        } else {
            self.promise_thunks.drop_fnsig
        };
        let Some((drop_fid, drop_sig)) = drop_pair else {
            return cb_op;
        };
        let wrap_user_sig = intern_fn_sig(self.fn_sigs, vec![Type::I64], Type::I64);
        let wrap_ty = Type::Closure(wrap_user_sig);
        let env_v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.obj_alloc,
                vec![Operand::ConstI64(PTHUNK_ENV_SIZE)],
            ),
            wrap_ty,
            None,
        );
        // Universal heap header: refcount=1, type_tag=CLOSURE=3.
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI32(1), Operand::Value(env_v), 0),
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI32(3), Operand::Value(env_v), 4),
        );
        let thunk_addr = self.f.append_inst(
            self.cur_block,
            InstKind::FnAddr(thunk_fid),
            Type::FnSig(thunk_sig),
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::Value(thunk_addr),
                Operand::Value(env_v),
                CLOSURE_FN_ADDR_OFF,
            ),
        );
        let drop_addr = self.f.append_inst(
            self.cur_block,
            InstKind::FnAddr(drop_fid),
            Type::FnSig(drop_sig),
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::Value(drop_addr),
                Operand::Value(env_v),
                CLOSURE_DROP_FN_OFF,
            ),
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::ConstI64(0),
                Operand::Value(env_v),
                CLOSURE_PROPS_OFF,
            ),
        );
        // Capture 0: the real callback. Its natural ref moves into
        // the wrap (the wrap's drop releases it), so the wrap takes
        // the callback's exact ownership position at the call site.
        self.f.append_void(
            self.cur_block,
            InstKind::Store(cb_op, Operand::Value(env_v), CLOSURE_CAP_BASE_OFF),
        );
        Operand::Value(env_v)
    }

    /* T-15.g.3 (v0.5.0) — `p.then(cb)` for built-in Promise.
     * MVP: cb is `(v: number) => number`. Lowers to a
     * runtime helper that:
     *   1. allocates a fresh result Promise (pending)
     *   2. heap-allocates a {source, cb, result} struct
     *   3. attaches the dispatcher to source's callbacks
     *   4. returns result Promise
     * The dispatcher reads source's resolved value via
     * __torajs_promise_get_value, calls cb, resolves
     * result. T-15.g.4 generalizes to non-i64 types and
     * Type::Closure (env-carrying) cb. */
    /// Returns None when the receiver is not a provably built-in
    /// Promise (user-class `.then` keeps the regular Member-call
    /// path). Extracted from lower_expr's Call arm (file-size
    /// known-debt).
    pub(crate) fn try_lower_promise_chain_call(
        &mut self,
        callee: ExprId,
        args: &[ExprId],
    ) -> Option<Operand> {
        let Expr::Member {
            obj: src_id,
            name: m_name,
        } = self.ast.get_expr(callee)
        else {
            return None;
        };
        if !((m_name == "then" || m_name == "catch" || m_name == "finally")
            && (args.len() == 1 || (m_name == "then" && args.len() == 2)))
        {
            return None;
        }
        // Static-type check (no eager lower) — same pattern
        // as the await Member dispatch. Only fire when src
        // is provably built-in Promise so user-class .then
        // keeps working through the regular Member-call path.
        let src_is_builtin_promise = match self.ast.get_expr(*src_id) {
            Expr::Ident(n) => self
                .locals
                .get(n)
                .map(|info| matches!(info.ty, Type::Promise))
                .unwrap_or(false),
            Expr::Call {
                callee: src_callee, ..
            } => {
                // Built-in Promise namespace statics. resolve/reject (T-15.g.5)
                // were the original entries; P10.2-A2 extends to all/race/any/
                // allSettled (T-17.a/b/c) so chained `.then`/`.catch`/`.finally`
                // on their results lowers through the runtime helpers instead
                // of the user-class fallback. check.rs already returns
                // Type::Promise for each, so all that's missing here is the
                // source-callee shape recognition.
                let static_ctor = matches!(
                    self.ast.get_expr(*src_callee),
                    Expr::Member { obj: ns_id, name: src_m }
                        if matches!(
                            src_m.as_str(),
                            "resolve" | "reject" | "all" | "race" | "any" | "allSettled"
                        ) && matches!(
                                self.ast.get_expr(*ns_id),
                                Expr::Ident(ns) if ns == "Promise"
                            )
                );
                // Chained `.then(...)` — its result is itself a
                // built-in Promise. Walks the callee shape but
                // does NOT require obj==Ident("Promise").
                let then_chain = matches!(
                    self.ast.get_expr(*src_callee),
                    Expr::Member { name: src_m, .. }
                        if src_m == "then" || src_m == "catch" || src_m == "finally"
                );
                // User fn whose declared return type is
                // Type::Promise (async desugar / Promise<T>
                // return annotation).
                let fn_returns_promise =
                    if let Expr::Ident(fn_name) = self.ast.get_expr(*src_callee) {
                        self.fn_table
                            .get(fn_name)
                            .copied()
                            .and_then(|fid| self.signatures.get(&fid).copied())
                            .map(|ty| matches!(ty, Type::Promise))
                            .unwrap_or(false)
                    } else {
                        false
                    };
                // T-19.g — fs/promises async returns +
                // Bun.file(...).text/.exists also produce
                // built-in Promise. Mirrors the
                // `await p.value` site's source detection
                // so `Bun.file(p).text().then(cb)` lowers
                // through the runtime helper instead of
                // bouncing off the user-class fallback.
                let fs_async = matches!(
                    self.ast.get_expr(*src_callee),
                    Expr::Member { obj: ns_id, name: m_name }
                        if matches!(
                            m_name.as_str(),
                            "readFile" | "writeFile" | "appendFile"
                                | "unlink" | "mkdir" | "exists" | "readdir"
                        ) && matches!(
                            self.ast.get_expr(*ns_id),
                            Expr::Ident(ns) if ns == "fs_promises"
                        )
                );
                let bun_file_text = matches!(
                    self.ast.get_expr(*src_callee),
                    Expr::Member { obj: file_id, name: m_name }
                        if (m_name == "text" || m_name == "exists")
                            && matches!(
                                self.ast.get_expr(*file_id),
                                Expr::Call { callee: f_callee, .. }
                                    if matches!(
                                        self.ast.get_expr(*f_callee),
                                        Expr::Member { obj: ns_id, name: fm }
                                            if fm == "file"
                                                && matches!(
                                                    self.ast.get_expr(*ns_id),
                                                    Expr::Ident(ns) if ns == "Bun"
                                                )
                                    )
                            )
                );
                static_ctor || then_chain || fn_returns_promise || fs_async || bun_file_text
            }
            _ => false,
        };
        if !src_is_builtin_promise {
            return None;
        }
        let src_op = self.lower_expr(*src_id);
        // T-19.l — 2-arg `.then(onOk, onErr)` form is
        // spec equivalent of `.then(onOk).catch(onErr)`.
        // Lower as a chained pair of helper calls; the
        // intermediate Promise is the bridge between
        // the two stages and gets dropped after the
        // catch attaches. Only fires for `.then` —
        // `.catch` / `.finally` are 1-arg only.
        let v = if m_name == "then" && args.len() == 2 {
            let on_ok = self.lower_expr(args[0]);
            let on_ok = self.maybe_wrap_promise_cb(on_ok);
            let on_err = self.lower_expr(args[1]);
            let on_err = self.maybe_wrap_promise_cb(on_err);
            let on_ok_ty = self.operand_ty(&on_ok);
            let then_fid = if matches!(on_ok_ty, Type::Closure(_)) {
                self.intrinsics.promise_then_closure
            } else {
                self.intrinsics.promise_then_simple
            };
            let mid = self.f.append_inst(
                self.cur_block,
                InstKind::Call(then_fid, vec![src_op.clone(), on_ok]),
                Type::Promise,
                None,
            );
            // ②.6b — pick the catch dispatcher by the (possibly
            // wrapped) handler's shape.
            let catch_fid = if matches!(self.operand_ty(&on_err), Type::Closure(_)) {
                self.intrinsics.promise_catch_closure
            } else {
                self.intrinsics.promise_catch_simple
            };
            let v = self.f.append_inst(
                self.cur_block,
                InstKind::Call(catch_fid, vec![Operand::Value(mid), on_err]),
                Type::Promise,
                None,
            );
            // The `mid` Promise is consumed by .catch
            // (which inc's its source's rc); drop the
            // chain's natural ref so the count balances.
            self.emit_drop_value(Operand::Value(mid), Type::Promise);
            v
        } else {
            let cb_op = self.lower_expr(args[0]);
            // ②.6b — f64-faced then/catch handlers get the bits-ABI
            // adapter (finally takes no value, nothing to adapt).
            let cb_op = if m_name == "finally" {
                cb_op
            } else {
                self.maybe_wrap_promise_cb(cb_op)
            };
            // T-15.g.5 / T-19.k / T-19.n — pick the
            // right runtime helper. All three method
            // names support both simple-fn and closure
            // cb shapes — selection by cb's static type
            // (Type::Closure → env-pointer dispatcher,
            // else → raw fn-pointer dispatcher).
            let cb_ty = self.operand_ty(&cb_op);
            let is_closure = matches!(cb_ty, Type::Closure(_));
            let then_intrinsic = match (m_name.as_str(), is_closure) {
                ("then", true) => self.intrinsics.promise_then_closure,
                ("then", false) => self.intrinsics.promise_then_simple,
                ("catch", true) => self.intrinsics.promise_catch_closure,
                ("catch", false) => self.intrinsics.promise_catch_simple,
                ("finally", true) => self.intrinsics.promise_finally_closure,
                ("finally", false) => self.intrinsics.promise_finally,
                _ => unreachable!(),
            };
            self.f.append_inst(
                self.cur_block,
                InstKind::Call(then_intrinsic, vec![src_op.clone(), cb_op]),
                Type::Promise,
                None,
            )
        };
        // T-15.g.7 — drop fresh source after .then.
        // Now that promise_drop is rc-aware AND
        // then_simple inc's source on attach, this
        // drop just balances the natural ref of the
        // intermediate `.then` result. Skip on
        // borrow-source (Ident / Member / Index —
        // owner still holds the ref).
        let src_is_borrow = matches!(
            self.ast.get_expr(*src_id),
            Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
        );
        if !src_is_borrow {
            self.emit_drop_value(src_op, Type::Promise);
        }
        return Some(Operand::Value(v));
    }
}
