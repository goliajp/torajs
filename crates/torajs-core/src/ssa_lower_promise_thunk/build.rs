//! The per-variant adapter + env-drop synthesis of
//! [`super`]/`ssa_lower_promise_thunk` — split out when RFC
//! 20260810-indirect-argc-abi S1's hidden-argc slot grew the parent
//! past the 500-line cap (same child-module shape as
//! `ssa_lower_boxed_entry/build.rs`).

use std::collections::HashMap;

use crate::ssa::{self, FuncId, InstKind, Module, Operand, Terminator, Type};
use crate::ssa_lower::{CLOSURE_CAP_BASE_OFF, CLOSURE_FN_ADDR_OFF};

/// One adapter: `(env, v_bits: i64) -> i64`. Loads the wrapped
/// callback from capture slot 0, bit-casts the value in per the
/// param face, calls it (closure inners pass their own env first),
/// bit-casts the result out per the ret face.
pub(super) fn build_thunk(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, ssa::SigId>,
    inner_closure: bool,
    p: bool,
    r: bool,
) -> (FuncId, ssa::SigId) {
    let name = format!(
        "__pthunk_{}_{}{}",
        if inner_closure { "c" } else { "f" },
        u8::from(p),
        u8::from(r)
    );
    let fid = FuncId(module.funcs.len() as u32);
    // S1 (RFC 20260810-indirect-argc-abi) — the thunk is itself an
    // `__env`-first entry (the promise runtime calls it through
    // `ThenClosureFn`), so it carries the hidden I64 argc at
    // position 1 like every lifted closure.
    let own_sig =
        crate::ssa_lower::intern_fn_sig(fn_sigs, vec![Type::Ptr, Type::I64, Type::I64], Type::I64);
    fn_table.insert(name.clone(), fid);
    fn_sig_ids.insert(fid, own_sig);
    let mut f = ssa::Function::new(&name, Type::I64);
    let env = f.add_param(Type::Ptr, "__env");
    let _argc = f.add_param(Type::I64, "__torajs_argc");
    let v = f.add_param(Type::I64, "v");
    let entry = f.add_block();
    let inner = f.append_inst(
        entry,
        InstKind::Load(Type::Ptr, Operand::Value(env), CLOSURE_CAP_BASE_OFF),
        Type::Ptr,
        None,
    );
    let p_ty = if p { Type::F64 } else { Type::I64 };
    let r_ty = if r { Type::F64 } else { Type::I64 };
    let v_in = if p {
        Operand::Value(f.append_inst(
            entry,
            InstKind::BitCastI64ToF64(Operand::Value(v)),
            Type::F64,
            None,
        ))
    } else {
        Operand::Value(v)
    };
    let raw = if inner_closure {
        let fnp = f.append_inst(
            entry,
            InstKind::Load(Type::Ptr, Operand::Value(inner), CLOSURE_FN_ADDR_OFF),
            Type::Ptr,
            None,
        );
        // S1 — the inner closure entry takes the hidden argc after
        // its env; the thunk always delivers exactly one value.
        let callee_sig =
            crate::ssa_lower::intern_fn_sig(fn_sigs, vec![Type::Ptr, Type::I64, p_ty], r_ty);
        f.append_inst(
            entry,
            InstKind::CallIndirect(
                callee_sig,
                Operand::Value(fnp),
                vec![Operand::Value(inner), Operand::ConstI64(1), v_in],
            ),
            r_ty,
            None,
        )
    } else {
        let callee_sig = crate::ssa_lower::intern_fn_sig(fn_sigs, vec![p_ty], r_ty);
        f.append_inst(
            entry,
            InstKind::CallIndirect(callee_sig, Operand::Value(inner), vec![v_in]),
            r_ty,
            None,
        )
    };
    let out = if r {
        f.append_inst(
            entry,
            InstKind::BitCastF64ToI64(Operand::Value(raw)),
            Type::I64,
            None,
        )
    } else {
        raw
    };
    f.set_term(entry, Terminator::Ret(Some(Operand::Value(out))));
    module.funcs.push(f);
    (fid, own_sig)
}

/// Env-drop for a wrap env: cycle-buffer scrub first (a closure-inner
/// wrap is collector-visitable once its trace_fn is set — a buffered
/// candidate that normal-drops here would leave a dangling buffer
/// entry, same protection as every other drop shape), then closure
/// inners release their captured callback ref; both variants free the
/// `PTHUNK_ENV_SIZE` block.
pub(super) fn build_env_drop(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, ssa::SigId>,
    obj_drop_sized_id: FuncId,
    value_drop_heap_id: Option<FuncId>,
    cycle_unbuffer_id: FuncId,
) -> (FuncId, ssa::SigId) {
    let name = if value_drop_heap_id.is_some() {
        "__pthunk_env_drop_c"
    } else {
        "__pthunk_env_drop_f"
    };
    let fid = FuncId(module.funcs.len() as u32);
    let sig = crate::ssa_lower::intern_fn_sig(fn_sigs, vec![Type::Ptr], Type::Void);
    fn_table.insert(name.to_string(), fid);
    fn_sig_ids.insert(fid, sig);
    let mut f = ssa::Function::new(name, Type::Void);
    let env = f.add_param(Type::Ptr, "env");
    let entry = f.add_block();
    f.append_void(
        entry,
        InstKind::Call(cycle_unbuffer_id, vec![Operand::Value(env)]),
    );
    if let Some(drop_heap) = value_drop_heap_id {
        let inner = f.append_inst(
            entry,
            InstKind::Load(Type::Ptr, Operand::Value(env), CLOSURE_CAP_BASE_OFF),
            Type::Ptr,
            None,
        );
        f.append_void(
            entry,
            InstKind::Call(drop_heap, vec![Operand::Value(inner)]),
        );
    }
    f.append_void(
        entry,
        InstKind::Call(
            obj_drop_sized_id,
            vec![
                Operand::Value(env),
                Operand::ConstI64(super::PTHUNK_ENV_SIZE),
            ],
        ),
    );
    f.set_term(entry, Terminator::Ret(None));
    module.funcs.push(f);
    (fid, sig)
}
