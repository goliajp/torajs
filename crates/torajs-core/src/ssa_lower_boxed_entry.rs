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

mod unbox;
use unbox::{BoxedCoerceIntrinsics, drop_obj_temps, drop_str_temps, unbox_args};

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

/// S-NEW 刀 2 — factory adapters exist only so
/// `__torajs_anyv_construct` can reach a class through a value. A
/// program that never constructs from a value gets none of them: one
/// extra function per class is a real cost to an artifact whose size
/// is a differentiator. r293 — `Reflect.construct` reaches the same
/// registry through its own kernel, so its call shape arms the
/// synthesis too.
fn program_constructs_from_value(ast: &Ast) -> bool {
    ast.exprs.iter().any(|e| match e {
        crate::ast::Expr::NewDynamic { .. } => true,
        crate::ast::Expr::Call { callee, .. } => matches!(
            ast.get_expr(*callee),
            crate::ast::Expr::Member { obj, name }
                if name == "construct"
                    && matches!(ast.get_expr(*obj), crate::ast::Expr::Ident(ns) if ns == "Reflect")
        ),
        _ => false,
    })
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
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, Type>,
) -> HashMap<FuncId, (FuncId, ssa::SigId)> {
    // S2.36 — sid → class_tag for the struct-param coercion call.
    // Named classes resolve through their name tag; anonymous shapes
    // through the Pass-1.5 stamp pool (same split the ObjectLit
    // alloc stamping uses).
    let named_sid_to_tag: HashMap<ssa::StructId, u32> = class_name_to_tag
        .iter()
        .filter_map(|(cname, &tag)| match aliases.get(cname) {
            Some(Type::Obj(sid)) => Some((*sid, tag)),
            _ => None,
        })
        .collect();
    let sid_tag = |sid: ssa::StructId| -> u32 {
        named_sid_to_tag
            .get(&sid)
            .copied()
            .unwrap_or_else(|| anon_stamp_pool.borrow_mut().assign_or_get(sid))
    };
    let constructs_from_value = program_constructs_from_value(ast);
    let mut targets: Vec<BoxedEntryTarget> = Vec::new();
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
        // RFC 20260717-class-first-class-value knife B cut 2 — static
        // method bodies (`__sm_<C>__<m>`) have NO env/this head param;
        // their adapter maps argv straight onto the user params and
        // drops its env argument (static dispatch carries no
        // receiver).
        let is_static = name.starts_with("__sm_");
        // S-NEW 刀 2 — the `__new_<C>` factories, same head-less shape
        // as a static method: their params ARE the constructor's, so
        // argv maps straight onto them. `__torajs_anyv_construct`
        // reaches a class object's factory through this adapter.
        let is_factory = constructs_from_value && name.starts_with("__new_");
        if !first_is_env && !first_is_this && !is_static && !is_factory {
            continue;
        }
        let feeds_env = first_is_env || first_is_this;
        let Some(&fid) = fn_table.get(name.as_str()) else {
            continue;
        };
        let Some(&sig_id) = fn_sig_ids.get(&fid) else {
            continue;
        };
        let (param_tys, ret_ty) = fn_sigs[sig_id.0 as usize].clone();
        let env_count = usize::from(feeds_env);
        // RFC 20260708-variadic — a body carrying the synthetic
        // `__torajs_real_argc` param (661/663 injection) takes the
        // adapter's argc VALUE in that slot; argv slots map to the
        // params after it. Pre-fix the adapter unboxed argv[0] into
        // the argc slot (latent mis-bind, unreachable while the 661
        // kill rules kept argc closures out of any-world).
        let has_real_argc = params
            .get(env_count)
            .is_some_and(|p| p.name == "__torajs_real_argc");
        // RFC 20260708-closure-argv-face — a full-arguments body
        // also carries the raw argv pointer right after the argc
        // slot; the adapter feeds its own argv argument there.
        let has_argv = params
            .get(env_count + 1)
            .is_some_and(|p| p.name == "__torajs_argv");
        // param 0 is the env Ptr (when the body carries one); the
        // boxed surface covers the rest (minus the argc/argv slots,
        // which never consume argv positions).
        let skip = env_count + usize::from(has_real_argc) + usize::from(has_argv);
        let user_tys = param_tys[skip..].to_vec();
        if user_tys.len() > MAX_BOXED_PARAMS {
            continue;
        }
        if !user_tys.iter().all(boxable) || !(ret_ty == Type::Void || boxable(&ret_ty)) {
            continue;
        }
        // S2.39 — literal defaults on the user params, positionally
        // aligned with `user_tys`. The adapter fills these when an
        // argv slot arrives undefined (missing OR explicit — ES
        // §10.2.1.3 treats both alike), which a runtime call cannot
        // get from the caller-side default injection. Only Number /
        // Bool literals qualify (an expression default may reference
        // prior params and needs real callee-side evaluation).
        let dflt_lits: Vec<Option<DfltLit>> = params[skip..]
            .iter()
            .map(|p| match p.default.map(|d| ast.get_expr(d)) {
                Some(crate::ast::Expr::Number(n)) => Some(DfltLit::Num(*n)),
                Some(crate::ast::Expr::Bool(b)) => Some(DfltLit::Bool(*b)),
                _ => None,
            })
            .collect();
        targets.push(BoxedEntryTarget {
            name: name.clone(),
            fid,
            user_tys,
            ret_ty,
            has_real_argc,
            has_argv,
            feeds_env,
            dflt_lits,
        });
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
    // S2.36 — struct-param coercion kernel + the release for its
    // owned answer, declared lazily like anyv_to_str.
    let arg_to_struct = crate::ssa_lower_synthesize_main::declare_intrinsic(
        module,
        fn_table,
        "__torajs_anyv_arg_to_struct",
        &[Type::Any, Type::I64],
        Type::Any,
    );
    let anyv_rc_dec = crate::ssa_lower_synthesize_main::declare_intrinsic(
        module,
        fn_table,
        "__torajs_anyv_rc_dec",
        &[Type::Any],
        Type::Void,
    );
    // S2.39 — the literal-default substitution kernel (answers the
    // slot unless undefined, else boxes the baked literal), declared
    // lazily like the trio above.
    let anyv_or_default = crate::ssa_lower_synthesize_main::declare_intrinsic(
        module,
        fn_table,
        "__torajs_anyv_or_default",
        &[Type::Any, Type::I64, Type::I64],
        Type::Any,
    );
    for t in targets {
        let obj_tags: Vec<Option<u32>> = t
            .user_tys
            .iter()
            .map(|ty| match ty {
                Type::Obj(sid) => Some(sid_tag(*sid)),
                _ => None,
            })
            .collect();
        let pair = build_boxed_entry(
            module,
            fn_table,
            fn_sigs,
            fn_sig_ids,
            intr,
            anyv_to_str,
            BoxedCoerceIntrinsics {
                arg_to_struct,
                anyv_rc_dec,
                anyv_or_default,
                obj_tags: &obj_tags,
            },
            &t.name,
            t.fid,
            &t.user_tys,
            t.ret_ty,
            t.has_real_argc,
            t.has_argv,
            t.feeds_env,
            &t.dflt_lits,
        );
        entries.insert(t.fid, pair);
    }
    entries
}

/// S2.39 — one qualifying literal default (`b = 39` / `flag = true`),
/// substituted by the adapter when the argv slot arrives undefined.
#[derive(Clone, Copy)]
pub(crate) enum DfltLit {
    Num(f64),
    Bool(bool),
}

/// One adapter-synthesis target — the per-FnDecl facts the targets
/// loop collects before any adapter is built.
struct BoxedEntryTarget {
    name: String,
    fid: FuncId,
    user_tys: Vec<Type>,
    ret_ty: Type,
    has_real_argc: bool,
    has_argv: bool,
    feeds_env: bool,
    dflt_lits: Vec<Option<DfltLit>>,
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
    coerce: BoxedCoerceIntrinsics<'_>,
    body_name: &str,
    body_fid: FuncId,
    user_tys: &[Type],
    ret_ty: Type,
    has_real_argc: bool,
    has_argv: bool,
    feeds_env: bool,
    dflt_lits: &[Option<DfltLit>],
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
    // S2.36 — coerced struct-param boxes (the kernel answers OWNED:
    // a stake on pass-throughs, a fresh cell for materialized
    // dynobjs) released after the body call via anyv_rc_dec (no-op
    // on immediates).
    let mut obj_temps: Vec<ssa::ValueId> = Vec::new();
    // A static-method body (`__sm_`) has no env slot — the adapter
    // drops its env argument (knife B cut 2).
    if feeds_env {
        args.push(Operand::Value(env));
    }
    // RFC 20260708-variadic — feed the dispatcher's real argc into
    // the body's synthetic `__torajs_real_argc` slot; the argv-face
    // body additionally takes the raw argv pointer.
    if has_real_argc {
        args.push(Operand::Value(argc));
    }
    if has_argv {
        args.push(Operand::Value(argv));
    }
    unbox_args(
        &mut f,
        entry,
        argv,
        user_tys,
        intr,
        anyv_to_str,
        &coerce,
        &mut args,
        &mut str_temps,
        &mut obj_temps,
        dflt_lits,
    );

    let boxed = if ret_ty == Type::Void {
        f.append_void(entry, InstKind::Call(body_fid, args));
        drop_str_temps(&mut f, entry, intr, &str_temps);
        drop_obj_temps(&mut f, entry, coerce.anyv_rc_dec, &obj_temps);
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
        drop_obj_temps(&mut f, entry, coerce.anyv_rc_dec, &obj_temps);
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
