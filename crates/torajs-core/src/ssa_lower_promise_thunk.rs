//! ②.6b (ann-width RFC §5.7) — promise callback ABI thunks.
//!
//! The promise runtime moves the resolved value as raw 8 bytes and
//! invokes handlers through one fixed signature — `(env, i64) -> i64`
//! (`torajs-promise/src/then.rs` `ThenClosureFn`). A callback whose
//! width-negotiated user signature carries an F64 face would put its
//! value in the wrong register bank across that boundary, so the
//! `.then` / `.catch` lowering wraps such callbacks in a synthesized
//! bits-ABI adapter: a closure-shaped env block whose fn slot points
//! at a thunk that bit-casts the value across the boundary and calls
//! the real callback (capture slot 0).
//!
//! Thunks are synthesized in Pass 1 (the module fn list is no longer
//! growable once per-fn lowering starts) — one per needed
//! `(inner-is-closure, param-f64, ret-f64)` variant, discovered by a
//! pre-scan of `.then` / `.catch` call sites against the width table.
//! Modules without a promise chain (every bench case) synthesize
//! nothing — zero artifact delta.

use crate::ast::{Ast, Expr, Stmt};
use crate::num_width::{SlotKey, WidthTable};
use crate::ssa::{self, FuncId, InstKind, Module, Operand, Type};
use crate::ssa_lower::{
    CLOSURE_CAP_BASE_OFF, CLOSURE_DROP_FN_OFF, CLOSURE_FN_ADDR_OFF, CLOSURE_PROPS_OFF,
    intern_fn_sig,
};
use std::collections::HashMap;

mod build;
use build::{build_env_drop, build_thunk};

/// Wrap-env byte size: closure header (`CLOSURE_CAP_BASE_OFF`)
/// plus one capture slot holding the wrapped callback value.
pub(crate) const PTHUNK_ENV_SIZE: i64 = CLOSURE_CAP_BASE_OFF as i64 + 8;

/// Synthesized adapters, keyed by
/// `(inner_is_closure, param_is_f64, ret_is_f64)`. `drop_fid` is the
/// matching env-drop for the wrap env (closure inners release their
/// capture; both variants free the `PTHUNK_ENV_SIZE` block).
/// `trace_closure` is the shared cap0 trace fn closure-inner wraps
/// store at `CLOSURE_TRACE_FN_OFF` so the cycle collector walks the
/// wrapped callback edge (RFC 20260717 residual ①); fnsig inners
/// hold a raw code address in cap0 — never a cell — and keep 0.
pub(crate) struct PromiseThunks {
    map: HashMap<(bool, bool, bool), (FuncId, ssa::SigId)>,
    pub(crate) drop_closure: Option<(FuncId, ssa::SigId)>,
    pub(crate) drop_fnsig: Option<(FuncId, ssa::SigId)>,
    pub(crate) trace_closure: Option<(FuncId, ssa::SigId)>,
}

impl PromiseThunks {
    pub(crate) fn get(
        &self,
        inner_closure: bool,
        p: bool,
        r: bool,
    ) -> Option<(FuncId, ssa::SigId)> {
        self.map.get(&(inner_closure, p, r)).copied()
    }
}

/// Pre-scan `.then` / `.catch` callback idents against the width
/// table and synthesize the needed adapter variants. Must run before
/// the `signatures` snapshot (thunks need ret-type hints like any fn).
#[allow(clippy::too_many_arguments)]
pub(crate) fn synthesize_promise_thunks(
    ast: &Ast,
    table: &WidthTable,
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, ssa::SigId>,
    obj_drop_sized_id: FuncId,
    value_drop_heap_id: FuncId,
    cycle_unbuffer_id: FuncId,
) -> PromiseThunks {
    let mut thunks = PromiseThunks {
        map: HashMap::new(),
        drop_closure: None,
        drop_fnsig: None,
        trace_closure: None,
    };
    // fn name → user param names (mirrors the analysis fn_params with
    // the lifted-closure `__env` slot stripped) plus the number-domain
    // gates for the first param / ret faces (same gate as the
    // analysis-side promise_chain_wiring — non-number faces never
    // join the width class, so querying them would be meaningless).
    let mut fn_user_params: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut fn_is_closure: HashMap<&str, bool> = HashMap::new();
    let mut fn_num_faces: HashMap<&str, (bool, bool)> = HashMap::new();
    for stmt in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } = stmt
        {
            let lifted = params.first().is_some_and(|p| p.name == "__env");
            let user = &params[usize::from(lifted)..];
            let names: Vec<&str> = user.iter().map(|p| p.name.as_str()).collect();
            let p_num = user
                .first()
                .is_none_or(|p0| matches!(p0.type_ann.as_deref(), None | Some("number")));
            // Ret gate — the None branch means "annotation absent"
            // which the effective-ret-ty pass reads as Void when the
            // body doesn't value-return. Wrapping such a cb with an
            // F64 ret face lets the thunk's I64-ret CallIndirect
            // disagree with the cb's Void sig — the result register
            // is undefined and downstream `.then` chains see NaN /
            // raw i64 garbage. So require the body actually produce a
            // return value when the ann says number-face-by-default.
            let r_num = match return_type.as_deref() {
                Some("number") => true,
                None => crate::ast::body_has_value_return(body),
                _ => false,
            };
            fn_user_params.insert(name, names);
            fn_is_closure.insert(name, lifted);
            fn_num_faces.insert(name, (p_num, r_num));
        }
    }
    let mut needed: Vec<(bool, bool, bool)> = Vec::new();
    for expr in &ast.exprs {
        let Expr::Call { callee, args } = expr else {
            continue;
        };
        let Expr::Member { name, .. } = ast.get_expr(*callee) else {
            continue;
        };
        if !matches!(name.as_str(), "then" | "catch") {
            continue;
        }
        for a in args.iter().take(2) {
            let cb = match ast.get_expr(*a) {
                Expr::Ident(n) if fn_user_params.contains_key(n.as_str()) => n.as_str(),
                Expr::Closure { fn_name, .. } if fn_user_params.contains_key(fn_name.as_str()) => {
                    fn_name.as_str()
                }
                _ => continue,
            };
            let (p_num, r_num) = fn_num_faces[cb];
            let p = p_num
                && fn_user_params[cb].first().is_some_and(|p0| {
                    table.slot_is_f64(&SlotKey::Param(cb.to_string(), p0.to_string()))
                });
            let r = r_num && table.slot_is_f64(&SlotKey::Ret(cb.to_string()));
            if p || r {
                let key = (fn_is_closure[cb], p, r);
                if !needed.contains(&key) {
                    needed.push(key);
                }
            }
        }
    }
    if needed.is_empty() {
        return thunks;
    }
    needed.sort();
    let any_closure_inner = needed.iter().any(|(c, _, _)| *c);
    for (inner_closure, p, r) in needed {
        let fid = build_thunk(module, fn_table, fn_sigs, fn_sig_ids, inner_closure, p, r);
        thunks.map.insert((inner_closure, p, r), fid);
    }
    thunks.drop_closure = Some(build_env_drop(
        module,
        fn_table,
        fn_sigs,
        fn_sig_ids,
        obj_drop_sized_id,
        Some(value_drop_heap_id),
        cycle_unbuffer_id,
    ));
    thunks.drop_fnsig = Some(build_env_drop(
        module,
        fn_table,
        fn_sigs,
        fn_sig_ids,
        obj_drop_sized_id,
        None,
        cycle_unbuffer_id,
    ));
    if any_closure_inner {
        // Shared cap0 trace for closure-inner wraps (RFC 20260717
        // residual ①) — the knife-2 synthesis over a single by-value
        // Closure capture yields exactly the wrap env's shape, so the
        // trace-body contract stays single-sourced.
        let name = "__pthunk_env_trace";
        let fid = FuncId(module.funcs.len() as u32);
        let sig = crate::ssa_lower::intern_fn_sig(
            fn_sigs,
            vec![Type::Ptr, Type::Ptr, Type::Ptr],
            Type::Void,
        );
        fn_table.insert(name.to_string(), fid);
        fn_sig_ids.insert(fid, sig);
        let visit_sig = crate::ssa_lower::intern_fn_sig(
            fn_sigs,
            vec![Type::I64, Type::Ptr, Type::Ptr, Type::Ptr],
            Type::Void,
        );
        let cap_sig = crate::ssa_lower::intern_fn_sig(fn_sigs, vec![Type::I64], Type::I64);
        let f = crate::ssa_lower_env_trace::synthesize_env_trace(
            name,
            &[(Type::Closure(cap_sig), false)],
            visit_sig,
        );
        module.funcs.push(f);
        thunks.trace_closure = Some((fid, sig));
    }
    thunks
}

/// The call-site half of the adapters above: wrap a callback whose
/// negotiated signature carries an F64 face in its synthesized
/// bits-ABI thunk.
///
/// Lives here rather than with the `.then` / `.catch` lowering
/// because it is the same concern as the thunks it selects from —
/// and because the chain file had three lines of headroom left under
/// the file-size limit, so growing it was not an option.
impl crate::ssa_lower::LowerCtx<'_> {
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
        // boxed_entry — the pthunk wrap is promise-internal, never
        // reachable as an any-world callee; stays 0.
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::ConstI64(0),
                Operand::Value(env_v),
                crate::ssa_lower::CLOSURE_BOXED_ENTRY_OFF,
            ),
        );
        // trace_fn — closure inners store the shared cap0 trace so
        // the collector walks the wrapped-callback edge (RFC 20260717
        // residual ①); fnsig inners hold a raw code address in cap0
        // (not a cell — a code address could even pass the collector's
        // cell-like gate) and keep 0. obj_alloc is plain malloc, so
        // the 0 must be stored explicitly either way.
        let trace_op = match self.promise_thunks.trace_closure {
            Some((tfid, tsig)) if is_closure => Operand::Value(self.f.append_inst(
                self.cur_block,
                InstKind::FnAddr(tfid),
                Type::FnSig(tsig),
                None,
            )),
            _ => Operand::ConstI64(0),
        };
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                trace_op,
                Operand::Value(env_v),
                crate::ssa_lower::CLOSURE_TRACE_FN_OFF,
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
}
