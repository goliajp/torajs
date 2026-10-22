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

use crate::ast::{Ast, Stmt};
use crate::num_width::{SlotKey, WidthTable};
use crate::ssa::{self, FuncId, Module, Type};
use crate::ssa_lower::CLOSURE_CAP_BASE_OFF;
use std::collections::HashMap;

mod build;
pub(crate) mod twins;
mod wrap;
use build::{build_env_drop, build_env_trace, build_thunk};

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
    /// The closures a `.then` / `.catch` call may hand over — the
    /// twin table's rows ([`twins`]).
    handlers: Vec<FuncId>,
    /// A `.then` / `.catch` argument names no declaration, so the
    /// closure it hands over is known only at run time.
    unnamed: bool,
    /// The forwarder the twin table's first word names.
    twin_run: Option<FuncId>,
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
        handlers: Vec::new(),
        unnamed: false,
        twin_run: None,
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
    // A handler named through a closure binding resolves to every
    // closure that binding may hold — the ones the width analysis wired
    // the handler's faces through — so whichever one the call site
    // hands over finds its adapter. One that names no declaration (a
    // field read, a fn-typed param) has faces only its slot's type
    // knows at lowering, so every variant is made; unused ones are
    // stripped at link.
    let mut needed: Vec<(bool, bool, bool)> = Vec::new();
    let mut unnamed = false;
    twins::then_handlers(ast, table, |cb| {
        if !fn_user_params.contains_key(cb) {
            unnamed = true;
            return;
        }
        if fn_is_closure[cb]
            && let Some(&fid) = fn_table.get(cb)
            && !thunks.handlers.contains(&fid)
        {
            thunks.handlers.push(fid);
        }
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
    });
    if unnamed {
        for (p, r) in [(false, true), (true, false), (true, true)] {
            for key in [(false, p, r), (true, p, r)] {
                if !needed.contains(&key) {
                    needed.push(key);
                }
            }
        }
    }
    thunks.unnamed = unnamed;
    if unnamed || !thunks.handlers.is_empty() {
        thunks.twin_run = Some(twins::build_twin_run(module, fn_table, fn_sigs, fn_sig_ids));
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
        thunks.trace_closure = Some(build_env_trace(module, fn_table, fn_sigs, fn_sig_ids));
    }
    thunks
}
