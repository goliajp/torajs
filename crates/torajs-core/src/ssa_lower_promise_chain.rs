//! ②.6b (ann-width RFC §5.7) — built-in Promise chain lowering:
//! `.then` / `.catch` / `.finally` call sites, the f64-face callback
//! wrap (see `ssa_lower_promise_thunk.rs` for the synthesized
//! adapters), and the promise value-point width query the await /
//! resolve sites share ([`value_width`]). Extracted from `ssa_lower.rs`
//! (file-size known-debt).

mod value_width;

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type, ValueId};

impl crate::ssa_lower::LowerCtx<'_> {
    /// Static-type check (no eager lower) — same pattern as the
    /// await Member dispatch. `true` iff `src_id` is provably a
    /// built-in Promise so user-class `.then` keeps working through
    /// the regular Member-call path. Recognised source shapes:
    /// Promise-typed local Ident; Promise namespace statics
    /// (resolve/reject per T-15.g.5, all/race/any/allSettled per
    /// P10.2-A2); chained `.then`/`.catch`/`.finally` result; user
    /// fn declared `-> Promise` (async desugar / annotation);
    /// fs/promises async fns + `Bun.file(...).text/.exists`
    /// (T-19.g, mirrors the `await p.value` source detection).
    pub(crate) fn src_is_builtin_promise(&self, src_id: ExprId) -> bool {
        // `(p as Promise<T>).then(…)` — the assertion changes no value.
        let src_id = self.peel_as_wrappers(src_id);
        match self.ast.get_expr(src_id) {
            // a top-level binding read from a function body is a global
            Expr::Ident(n) => matches!(
                self.locals
                    .get(n)
                    .map(|info| &info.ty)
                    .or_else(|| self.globals.get(n)),
                Some(Type::Promise)
            ),
            Expr::Call {
                callee: src_callee, ..
            } => {
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
                // RFC 20260713 blade 3 — closure-valued callee whose
                // signature returns Promise: a lifted async arrow /
                // fn-expr bound to a local or K.3-promoted global
                // (`const f = async () => ...; f().then(...)`), or an
                // IIFE directly on the closure value
                // (`(async () => ...)().then(...)`).
                let closure_returns_promise = match self.ast.get_expr(*src_callee) {
                    Expr::Ident(n) => {
                        let slot_ty = self
                            .locals
                            .get(n)
                            .map(|info| &info.ty)
                            .or_else(|| self.globals.get(n));
                        matches!(
                            slot_ty,
                            Some(Type::Closure(sig)) | Some(Type::FnSig(sig))
                                if matches!(self.fn_sigs[sig.0 as usize].1, Type::Promise)
                        )
                    }
                    Expr::Closure { fn_name, .. } => self
                        .fn_table
                        .get(fn_name)
                        .copied()
                        .and_then(|fid| self.signatures.get(&fid).copied())
                        .map(|ty| matches!(ty, Type::Promise))
                        .unwrap_or(false),
                    _ => false,
                };
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
                // rotation 233 — the checker's answer covers every
                // call shape the enumerations above spell out one by
                // one, plus the ones they miss (an async-generator's
                // `gen().next()` / `.return()` / `.throw()` method
                // trio, any struct-method returning Promise). The
                // shape probes stay first: they also answer inside
                // synthesized fns whose exprs the checker never
                // visited.
                static_ctor
                    || then_chain
                    || fn_returns_promise
                    || closure_returns_promise
                    || fs_async
                    || bun_file_text
                    || matches!(
                        self.expr_types.get(&src_id),
                        Some(crate::check::Type::Promise(_))
                    )
            }
            // A field or element read. A `Promise`-typed field holds
            // the same built-in promise a `Promise`-typed binding
            // does, and the Ident arm above already answers for the
            // binding by asking its slot — so `const p = f();
            // p.then(cb)` chained while `c.p.then(cb)` fell through to
            // the resolve_callee panic ("unsupported member call
            // shape: then"). `expr_types` is the same question asked
            // of a field.
            //
            // An INDEX read is the same question again, and was the
            // same casualty: `const ps: Promise<number>[] = [...];
            // ps[0].then(cb)` reached that panic while the identical
            // program written `const p = ps[0]; p.then(cb)` lowered —
            // the element of a `Promise<T>[]` is as much a built-in
            // promise as a field of that type is.
            Expr::Member { .. } | Expr::OptChain { .. } | Expr::Index { .. } => matches!(
                self.expr_types.get(&src_id),
                Some(crate::check::Type::Promise(_))
            ),
            _ => false,
        }
    }

    // lower_then_two_arg + chain_cb_ret_repr live in the sibling
    // `ssa_lower_promise_chain_two_arg.rs` (knife 3 file-size split).

    /// 1-arg `.then` / `.catch` / `.finally` — pick the right
    /// runtime helper (T-15.g.5 / T-19.k / T-19.n). All three
    /// method names support both simple-fn and closure cb shapes —
    /// selection by cb's static type (Type::Closure → env-pointer
    /// dispatcher, else → raw fn-pointer dispatcher). ②.6b —
    /// f64-faced then/catch handlers get the bits-ABI adapter
    /// (finally takes no value, nothing to adapt).
    /// `cb_op` is the already-lowered handler: the caller lowers the
    /// arguments once, ahead of the own-`then` probe split (rotation
    /// 652), so both blocks share the one evaluation.
    fn lower_chain_one_arg(
        &mut self,
        m_name: &str,
        src_op: Operand,
        aid: ExprId,
        cb_pre: Operand,
    ) -> ValueId {
        // Before the bits-ABI wrapper replaces it — the wrap's own
        // signature is `(i64) -> i64`, so it can answer neither what
        // the handler's parameter is nor what its return is. See
        // `chain_cb_repr_word` / `chain_cb_param_repr`.
        let cb_pre_ty = self.operand_ty(&cb_pre);
        let handler = self.prepare_chain_handler(aid, cb_pre, m_name != "finally");
        let cb_op = handler.passed;
        let cb_ty = self.operand_ty(&cb_op);
        let is_closure = matches!(cb_ty, Type::Closure(_));
        let then_intrinsic = match (m_name, is_closure) {
            ("then", true) => self.intrinsics.promise_then_closure,
            ("then", false) => self.intrinsics.promise_then_simple,
            ("catch", true) => self.intrinsics.promise_catch_closure,
            ("catch", false) => self.intrinsics.promise_catch_simple,
            ("finally", true) => self.intrinsics.promise_finally_closure,
            ("finally", false) => self.intrinsics.promise_finally,
            _ => unreachable!(),
        };
        // knife 3 — then/catch carry the cb-return repr for the
        // kernel's result stamp. RFC 20260720-promise-any-cb knife 1
        // — bit 8 marks an any-param handler (kernel boxes per the
        // source's repr stamp; an UNSTAMPED source throws at attach,
        // so the call gets a throw-check).
        //
        // `finally` carries the RETURN half only: §27.2.5.3 runs the
        // handler argument-free, but WAITS on a thenable it returns,
        // so the kernel has to know whether the return register holds
        // anything and in what form. Nothing to unbox on the way in,
        // hence no param word and no throw-check.
        let repr_word = if m_name == "finally" {
            self.chain_cb_repr_word(aid, &cb_pre_ty)
        } else {
            self.chain_cb_repr_word(aid, &cb_pre_ty) | self.chain_cb_param_repr(&cb_pre_ty)
        };
        let mut call_args = vec![src_op, cb_op];
        call_args.push(Operand::ConstI64(repr_word));
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(then_intrinsic, call_args),
            Type::Promise,
            None,
        );
        if repr_word & crate::ssa_lower_promise_repr_mark::PARAM_ANY_FLAG != 0 {
            self.emit_throw_check(None);
        }
        self.settle_chain_handler(handler);
        v
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
            && (args.len() == 1
                || (m_name == "then" && args.len() == 2)
                || (m_name != "finally" && args.is_empty())))
        {
            return None;
        }
        if !self.src_is_builtin_promise(*src_id) {
            return None;
        }
        let src_op = self.lower_expr(*src_id);
        let mut arg_ops: Vec<Operand> = Vec::with_capacity(args.len());
        for &a in args {
            arg_ops.push(self.lower_expr(a));
        }
        // Rotation 652 — a `.then` on a cell carrying its OWN `then`
        // is an Invoke of that override (§27.2.5.4 is only the
        // builtin's body); the sibling probes the cell and splits.
        // §27.2.5.1 — the override answers whatever it returns, which
        // need not be a promise. In statement position there is no
        // slot for it to not fit in, so the sibling's typed return
        // contract is skipped there (the value is dropped instead).
        let discarded = self.discarded_stmt_call == Some(callee);
        let v = crate::ssa_lower_promise_chain_own_then::lower_chain_with_own_then_probe(
            self, src_op, *src_id, m_name, args, &arg_ops, discarded,
        );
        // T-15.g.7 — drop fresh source after .then.
        // Now that promise_drop is rc-aware AND
        // then_simple inc's source on attach, this
        // drop just balances the natural ref of the
        // intermediate `.then` result. Skip on
        // borrow-source (Ident / Member / Index —
        // owner still holds the ref).
        let src_is_borrow = matches!(
            self.ast.get_expr(self.peel_as_wrappers(*src_id)),
            Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
        );
        if !src_is_borrow {
            self.emit_drop_value(src_op, Type::Promise);
        }
        Some(Operand::Value(v))
    }

    /// The builtin lane of a chain call over already-lowered handler
    /// operands. 2-arg form only fires for `.then` — `.catch` /
    /// `.finally` are 1-arg only. The 0-arg form (`p.then()` /
    /// `p.catch()`, §27.2.5.4 with both handlers absent) rides the
    /// passthrough kernel: the derived promise adopts the source
    /// (Identity / Thrower semantics, one reaction tick).
    pub(crate) fn lower_chain_fast(
        &mut self,
        m_name: &str,
        src_op: Operand,
        args: &[ExprId],
        arg_ops: &[Operand],
    ) -> ValueId {
        if arg_ops.is_empty() {
            let cur_block = self.cur_block;
            self.f.append_inst(
                cur_block,
                InstKind::Call(self.intrinsics.promise_then_passthrough, vec![src_op]),
                Type::Promise,
                None,
            )
        } else if m_name == "then" && arg_ops.len() == 2 {
            self.lower_then_two_arg(src_op, args, arg_ops[0], arg_ops[1])
        } else {
            self.lower_chain_one_arg(m_name, src_op, args[0], arg_ops[0])
        }
    }
}
