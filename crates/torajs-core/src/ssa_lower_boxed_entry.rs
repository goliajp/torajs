//! Any-method-call RFC 20260704 C3a-2 — boxed dual-entry synthesis.
//!
//! For every lifted closure body fn the module carries, synthesize a
//! `__boxed_<name>(env, argv: *const AnyValue, argc) -> AnyValue`
//! adapter (the V8 JSFunction code-entry analogue): unbox each
//! argument slot per the body's static parameter type, call the body
//! directly (known FuncId — zero indirection), box the native return.
//! The closure construction site stores the adapter's address at
//! `CLOSURE_BOXED_ENTRY_OFF` so any-world dynamic call sites
//! (`o.f(x)` through a dynobj, Array HO callbacks) dispatch through
//! one uniform ABI without knowing the native signature.
//!
//! Synthesized in Pass 1 (`setup_callable_infra`, the same freeze
//! point as the promise thunks — the module fn list is no longer
//! growable once per-fn lowering starts). Modules without closures
//! synthesize nothing.
//!
//! Bounds (entry stays 0 → the runtime answers a catchable
//! TypeError, never a silent wrong call):
//! - more than [`MAX_BOXED_PARAMS`] user params (the runtime
//!   dispatcher hands the adapter a fixed 8-slot undefined-filled
//!   argv);
//! - a param/ret type outside the boxable set (struct-by-value etc.).
//!
//! Argument ledger: argv slots are BORROWED (the dynamic caller owns
//! them); unboxed heap pointers pass through as borrows — the native
//! callee's usual caller-keeps-args-alive convention. The native
//! return (cells +1 owned) transfers into the box, which the adapter
//! returns still owned — matching the boxed-value convention the
//! dispatcher's callers expect.

use std::collections::HashMap;

use crate::ast::{Ast, Stmt};
use crate::ssa::{self, FuncId, InstKind, Module, Operand, Terminator, Type};
use crate::ssa_lower::intern_fn_sig;

/// The runtime dispatcher's fixed argv width (undefined-filled).
pub(crate) const MAX_BOXED_PARAMS: usize = 8;

/// Intrinsic FuncIds the adapters call — a narrow slice of
/// `Intrinsics` so Pass 1 doesn't need the whole table.
pub(crate) struct BoxedEntryIntrinsics {
    /// `__torajs_anyv_box_from_pair(tag, value) -> AnyValue`.
    pub(crate) any_box: FuncId,
    /// `__torajs_anyv_to_number(v) -> f64` (ES ToNumber).
    pub(crate) any_to_number: FuncId,
    /// `__torajs_anyv_to_bool(v) -> bool` (ES ToBoolean).
    pub(crate) any_to_bool: FuncId,
    /// `__torajs_anyv_unbox_value(v) -> i64` (cell ptr bits).
    pub(crate) any_unbox_value: FuncId,
    /// `__torajs_str_drop(s)` — releases the Str-param ToString
    /// temps (see `anyv_to_str` below, declared lazily — the
    /// intrinsics table only carries the pair-ABI
    /// `__torajs_anyv_to_str_pair`).
    pub(crate) str_drop: FuncId,
}

/// `true` iff the type can cross the boxed boundary in either
/// direction with the intrinsics above. `Substr` params stay
/// unboxable (the layout differs from the owned Str `anyv_to_str`
/// yields — entry stays 0, catchable TypeError).
fn boxable(ty: &Type) -> bool {
    !matches!(ty, Type::Substr)
        && (matches!(
            ty,
            Type::Any | Type::I64 | Type::I32 | Type::F64 | Type::Bool | Type::Ptr
        ) || ty.is_refcounted())
}

/// Walk the lifted closure fns and synthesize one adapter each.
/// Returns `body fid → (adapter fid, adapter sig)` for the closure
/// construction sites.
pub(crate) fn synthesize_boxed_entries(
    ast: &Ast,
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, ssa::SigId>,
    intr: &BoxedEntryIntrinsics,
) -> HashMap<FuncId, (FuncId, ssa::SigId)> {
    let mut targets: Vec<(String, FuncId, Vec<Type>, Type, bool, bool)> = Vec::new();
    for stmt in &ast.stmts {
        let Stmt::FnDecl { name, params, .. } = stmt else {
            continue;
        };
        // Lifted closure bodies carry `__env` first; class-method
        // bodies (`__cm_<C>__<m>`) carry `__this` first — 刀 4 (RFC
        // 20260714-t262-top-clusters) reifies those too, so an
        // any-held class instance can dispatch its methods by name
        // through the class-methods table. Both are pointer-shaped
        // first params the adapter feeds its env argument into.
        let first_is_env = params.first().is_some_and(|p| p.name == "__env");
        let first_is_this =
            params.first().is_some_and(|p| p.name == "__this") && name.starts_with("__cm_");
        if !first_is_env && !first_is_this {
            continue;
        }
        let Some(&fid) = fn_table.get(name.as_str()) else {
            continue;
        };
        let Some(&sig_id) = fn_sig_ids.get(&fid) else {
            continue;
        };
        let (param_tys, ret_ty) = fn_sigs[sig_id.0 as usize].clone();
        // RFC 20260708-variadic — a body carrying the synthetic
        // `__torajs_real_argc` param (661/663 injection) takes the
        // adapter's argc VALUE in that slot; argv slots map to the
        // params after it. Pre-fix the adapter unboxed argv[0] into
        // the argc slot (latent mis-bind, unreachable while the 661
        // kill rules kept argc closures out of any-world).
        let has_real_argc = params
            .get(1)
            .is_some_and(|p| p.name == "__torajs_real_argc");
        // RFC 20260708-closure-argv-face — a full-arguments body
        // also carries the raw argv pointer right after the argc
        // slot; the adapter feeds its own argv argument there.
        let has_argv = params.get(2).is_some_and(|p| p.name == "__torajs_argv");
        // param 0 is the env Ptr; the boxed surface covers the rest
        // (minus the argc/argv slots, which never consume argv
        // positions).
        let skip = 1 + usize::from(has_real_argc) + usize::from(has_argv);
        let user_tys = param_tys[skip..].to_vec();
        if user_tys.len() > MAX_BOXED_PARAMS {
            continue;
        }
        if !user_tys.iter().all(boxable) || !(ret_ty == Type::Void || boxable(&ret_ty)) {
            continue;
        }
        targets.push((name.clone(), fid, user_tys, ret_ty, has_real_argc, has_argv));
    }
    let mut entries = HashMap::new();
    if targets.is_empty() {
        return entries;
    }
    // Single-value ToString (the intrinsics table only carries the
    // pair ABI) — declared once, lazily with the first adapter.
    let anyv_to_str = crate::ssa_lower_synthesize_main::declare_intrinsic(
        module,
        fn_table,
        "__torajs_anyv_to_str",
        &[Type::Any],
        Type::Ptr,
    );
    for (name, fid, user_tys, ret_ty, has_real_argc, has_argv) in targets {
        let pair = build_boxed_entry(
            module,
            fn_table,
            fn_sigs,
            fn_sig_ids,
            intr,
            anyv_to_str,
            &name,
            fid,
            &user_tys,
            ret_ty,
            has_real_argc,
            has_argv,
        );
        entries.insert(fid, pair);
    }
    entries
}

/// One adapter — see module doc for the ABI.
#[allow(clippy::too_many_arguments)]
fn build_boxed_entry(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, ssa::SigId>,
    intr: &BoxedEntryIntrinsics,
    anyv_to_str: FuncId,
    body_name: &str,
    body_fid: FuncId,
    user_tys: &[Type],
    ret_ty: Type,
    has_real_argc: bool,
    has_argv: bool,
) -> (FuncId, ssa::SigId) {
    let name = format!("__boxed_{body_name}");
    let fid = FuncId(module.funcs.len() as u32);
    let own_sig = intern_fn_sig(fn_sigs, vec![Type::Ptr, Type::Ptr, Type::I64], Type::Any);
    fn_table.insert(name.clone(), fid);
    fn_sig_ids.insert(fid, own_sig);
    let mut f = ssa::Function::new(&name, Type::Any);
    let env = f.add_param(Type::Ptr, "__env");
    let argv = f.add_param(Type::Ptr, "argv");
    let argc = f.add_param(Type::I64, "argc");
    let entry = f.add_block();

    let mut args: Vec<Operand> = Vec::with_capacity(user_tys.len() + 2);
    // Str-param temps (owned via ToString — a ShortStr argument
    // materializes) released after the body call.
    let mut str_temps: Vec<ssa::ValueId> = Vec::new();
    args.push(Operand::Value(env));
    // RFC 20260708-variadic — feed the dispatcher's real argc into
    // the body's synthetic `__torajs_real_argc` slot; the argv-face
    // body additionally takes the raw argv pointer.
    if has_real_argc {
        args.push(Operand::Value(argc));
    }
    if has_argv {
        args.push(Operand::Value(argv));
    }
    for (i, pty) in user_tys.iter().enumerate() {
        let av = Operand::Value(f.append_inst(
            entry,
            InstKind::Load(Type::Any, Operand::Value(argv), (i as u64) * 8),
            Type::Any,
            None,
        ));
        let arg = match pty {
            Type::Any => av,
            Type::F64 => Operand::Value(f.append_inst(
                entry,
                InstKind::Call(intr.any_to_number, vec![av]),
                Type::F64,
                None,
            )),
            // Width-narrowed number params truncate toward zero —
            // the narrowing proof can't see runtime any-calls, so a
            // fractional argument here is the RFC's documented
            // width-vs-any boundary (fixture keeps to integers).
            Type::I64 | Type::I32 => {
                let d = f.append_inst(
                    entry,
                    InstKind::Call(intr.any_to_number, vec![av]),
                    Type::F64,
                    None,
                );
                Operand::Value(f.append_inst(
                    entry,
                    InstKind::FpToSi(Operand::Value(d)),
                    Type::I64,
                    None,
                ))
            }
            Type::Bool => Operand::Value(f.append_inst(
                entry,
                InstKind::Call(intr.any_to_bool, vec![av]),
                Type::Bool,
                None,
            )),
            // Str params go through ToString (owned) — a borrow
            // unbox would leak the ShortStr materialization.
            Type::Str => {
                let s = f.append_inst(
                    entry,
                    InstKind::Call(anyv_to_str, vec![av]),
                    Type::Ptr,
                    None,
                );
                str_temps.push(s);
                Operand::Value(s)
            }
            // Other heap-typed / raw-ptr params: the cell's NaN-box
            // encoding is its pointer bits (borrow — the argv slot
            // keeps the reference alive across the call).
            _ => {
                let raw = f.append_inst(
                    entry,
                    InstKind::Call(intr.any_unbox_value, vec![av]),
                    Type::I64,
                    None,
                );
                Operand::Value(f.append_inst(
                    entry,
                    InstKind::IntToPtr(Operand::Value(raw)),
                    Type::Ptr,
                    None,
                ))
            }
        };
        args.push(arg);
    }

    let boxed = if ret_ty == Type::Void {
        f.append_void(entry, InstKind::Call(body_fid, args));
        drop_str_temps(&mut f, entry, intr, &str_temps);
        // undefined — tag 5.
        f.append_inst(
            entry,
            InstKind::Call(
                intr.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        )
    } else {
        let r = f.append_inst(entry, InstKind::Call(body_fid, args), ret_ty, None);
        drop_str_temps(&mut f, entry, intr, &str_temps);
        match ret_ty {
            Type::Any => r,
            Type::F64 => {
                let bits = f.append_inst(
                    entry,
                    InstKind::BitCastF64ToI64(Operand::Value(r)),
                    Type::I64,
                    None,
                );
                f.append_inst(
                    entry,
                    InstKind::Call(
                        intr.any_box,
                        vec![Operand::ConstI64(3), Operand::Value(bits)],
                    ),
                    Type::Any,
                    None,
                )
            }
            Type::Bool => {
                let z = f.append_inst(
                    entry,
                    InstKind::ZExtBoolToI64(Operand::Value(r)),
                    Type::I64,
                    None,
                );
                f.append_inst(
                    entry,
                    InstKind::Call(intr.any_box, vec![Operand::ConstI64(1), Operand::Value(z)]),
                    Type::Any,
                    None,
                )
            }
            Type::I64 | Type::I32 => f.append_inst(
                entry,
                InstKind::Call(intr.any_box, vec![Operand::ConstI64(2), Operand::Value(r)]),
                Type::Any,
                None,
            ),
            // Heap-typed returns arrive +1 owned; the box transfers
            // that reference to the returned AnyValue.
            _ => f.append_inst(
                entry,
                InstKind::Call(intr.any_box, vec![Operand::ConstI64(4), Operand::Value(r)]),
                Type::Any,
                None,
            ),
        }
    };
    f.set_term(entry, Terminator::Ret(Some(Operand::Value(boxed))));
    module.funcs.push(f);
    (fid, own_sig)
}

/// Release the Str-param ToString temps after the body call. A body
/// that returned one of them holds its own reference (owned-return
/// convention), so the release never frees a live result.
fn drop_str_temps(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    intr: &BoxedEntryIntrinsics,
    temps: &[ssa::ValueId],
) {
    for &t in temps {
        f.append_void(
            entry,
            InstKind::Call(intr.str_drop, vec![Operand::Value(t)]),
        );
    }
}
