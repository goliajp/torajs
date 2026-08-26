//! Env-drop fn infrastructure setup, drained from `lower_inner`
//! (chunk-333 RFC continuation):
//!
//! 1. **Per-closure pre-allocation** — every FnDecl whose first param is
//!    `__env` (lifted arrow OR synthesized forwarder for mixed-return
//!    wrapping) gets a paired `__env_drop_<name>` FuncId. Pre-registered
//!    here so Pass-2 closure-construction sites can `FnAddr(drop_fid)`
//!    and store it at env+8; the body is filled in by Pass 2.5 once
//!    `closure_captures` is populated.
//!
//! 2. **Trivial drop fn** — the no-capture closure-wrapper drop fn
//!    `__env_drop_trivial`. Used by the Return arm when wrapping a
//!    top-level FnAddr (Type::FnSig) into a Closure-typed value to
//!    satisfy a fn signature that returns `(...) => R`. The wrapper env
//!    has just the closure header (`fn_addr@8 + drop_fn@16 + props@24 +
//!    boxed_entry@32 + trace_fn@40`), no captures; the drop body just
//!    frees the env block.

use std::collections::HashMap;

use crate::ast::Stmt;
use crate::ssa::{self, FuncId, InstKind, Module, Operand, Terminator, Type};
use crate::ssa_lower::{CLOSURE_CAP_BASE_OFF, intern_fn_sig};
use crate::ssa_lower_intrinsics_init_a::InitA;

pub(crate) struct EnvDropSetup {
    pub env_drop_fids: Vec<(String, FuncId, ssa::SigId)>,
    pub env_drop_trivial_fid: (FuncId, ssa::SigId),
    /// RFC 20260717 closure-env-cycle knife 2 — the paired
    /// `__env_trace_<name>` FuncIds, populated in Pass 2.5 alongside
    /// the drop bodies from the same `closure_captures` truth.
    pub env_trace_fids: Vec<(String, FuncId)>,
}

pub(crate) fn run(
    ast: &crate::ast::Ast,
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, ssa::SigId>,
    init_a: &InitA,
) -> EnvDropSetup {
    let mut env_drop_fids: Vec<(String, FuncId, ssa::SigId)> = Vec::new();
    let mut env_trace_fids: Vec<(String, FuncId)> = Vec::new();
    for stmt in &ast.stmts {
        // Any FnDecl with `__env` as its first param is a closure-
        // shaped body (lifted arrow OR synthesized forwarder for
        // mixed-return wrapping). Each gets a paired env-drop fn.
        if let Stmt::FnDecl { name, params, .. } = stmt
            && params.first().is_some_and(|p| p.name == "__env")
        {
            let drop_name = format!("__env_drop_{name}");
            let fid = FuncId(module.funcs.len() as u32);
            fn_table.insert(drop_name.clone(), fid);
            let drop_sig = intern_fn_sig(fn_sigs, vec![Type::Ptr], Type::Void);
            fn_sig_ids.insert(fid, drop_sig);
            module
                .funcs
                .push(ssa::Function::new(&drop_name, Type::Void));
            env_drop_fids.push((name.clone(), fid, drop_sig));
            // Paired trace fn (RFC 20260717 knife 2). Registered
            // unconditionally — captures aren't known until Pass 2
            // and the fn list freezes at the signatures snapshot;
            // untraceable closures get an empty body and store 0 in
            // the env slot instead.
            let trace_name = format!("__env_trace_{name}");
            let tfid = FuncId(module.funcs.len() as u32);
            fn_table.insert(trace_name.clone(), tfid);
            let trace_sig =
                intern_fn_sig(fn_sigs, vec![Type::Ptr, Type::Ptr, Type::Ptr], Type::Void);
            fn_sig_ids.insert(tfid, trace_sig);
            module
                .funcs
                .push(ssa::Function::new(&trace_name, Type::Void));
            env_trace_fids.push((name.clone(), tfid));
        }
    }

    let env_drop_trivial_fid = {
        let fid = FuncId(module.funcs.len() as u32);
        fn_table.insert("__env_drop_trivial".into(), fid);
        let sig = intern_fn_sig(fn_sigs, vec![Type::Ptr], Type::Void);
        fn_sig_ids.insert(fid, sig);
        let mut f = ssa::Function::new("__env_drop_trivial", Type::Void);
        let env_pid = f.add_param(Type::Ptr, "env");
        let entry = f.add_block();
        // Trivial env wrapper: no captures, env block size = closure
        // header only (`fn_addr@8 + drop_fn@16 + props@24 +
        // boxed_entry@32 + trace_fn@40` = `CLOSURE_CAP_BASE_OFF`).
        // Same two seamed legs as the synthesized drop bodies — a
        // trivial env still shares the +24 expando slot (accessor
        // named-fn mints take `f.x = v` writes like any closure).
        let after_props = crate::ssa_lower_env_drop_and_ret_ty::emit_env_drop_prologue(
            &mut f,
            entry,
            env_pid,
            init_a.obj_capture.closure_unbuffer_slow,
            init_a.obj_capture.closure_drop_props_slow,
        );
        f.append_void(
            after_props,
            InstKind::Call(
                init_a.obj_capture.obj_drop_sized,
                vec![
                    Operand::Value(env_pid),
                    Operand::ConstI64(CLOSURE_CAP_BASE_OFF as i64),
                ],
            ),
        );
        f.set_term(after_props, Terminator::Ret(None));
        module.funcs.push(f);
        (fid, sig)
    };

    EnvDropSetup {
        env_drop_fids,
        env_drop_trivial_fid,
        env_trace_fids,
    }
}
