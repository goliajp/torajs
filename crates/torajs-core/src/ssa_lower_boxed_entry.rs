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

use crate::ast::Ast;
use crate::ssa::{self, FuncId, Module, Type};

mod unbox;
use unbox::BoxedCoerceIntrinsics;

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
    /// `__torajs_anyv_unbox_settle(v, raw)` — reclaims the ShortStr
    /// materialization a borrow-shaped `any_unbox_value` minted
    /// (no-op otherwise); the heap/ptr param legs emit it after the
    /// body call so the borrow stays live across it.
    pub(crate) any_unbox_settle: FuncId,
    /// `__torajs_str_drop(s)` — releases the Str-param ToString
    /// temps (see `anyv_to_str` below, declared lazily — the
    /// intrinsics table only carries the pair-ABI
    /// `__torajs_anyv_to_str_pair`).
    pub(crate) str_drop: FuncId,
    /// `__torajs_arr_alloc_any(cap) -> Ptr` — the rest-param
    /// adapter's fresh Arr<Any>.
    pub(crate) arr_alloc_any: FuncId,
    /// `__torajs_arr_any_push(arr, argv, argc, recv_slot) -> len` —
    /// fills it from the trailing argv window (incs stored cells;
    /// argv slots stay borrowed).
    pub(crate) arr_any_push: FuncId,
    /// `__torajs_anyv_or_default(v, tag, bits)` — the S2.39
    /// literal-default substitution kernel, shared with the
    /// direct-call terminal (S3.8) through the intrinsics table.
    pub(crate) anyv_or_default: FuncId,
    /// `__torajs_arr_mark_kind(arr, chain)` — stamps a typed array's
    /// element-kind chain so the any lane can decode its slots. The
    /// adapter's array RETURN needs it for the same reason an array
    /// ARGUMENT does (RFC 20260823-proxy-substrate 刀 4b).
    pub(crate) arr_mark_kind: FuncId,
}

/// Walk the lifted closure fns and synthesize one adapter each.
/// Returns `body fid → (adapter fid, adapter sig)` for the closure
/// construction sites.
pub(crate) fn synthesize_boxed_entries(
    ast: &Ast,
    call_retargets: &crate::ssa_lower::CallRetargets,
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, ssa::SigId>,
    intr: &BoxedEntryIntrinsics,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, Type>,
    arr_layouts: &[Type],
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
    let targets = collect_boxed_targets(&TargetTables {
        ast,
        call_retargets,
        fn_table,
        fn_sigs,
        fn_sig_ids,
        arr_layouts,
    });
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
    // 刀 2 (RFC 20260815-fn-value-rest-spread) — the typed-rest
    // conversion kernel, declared lazily like anyv_to_str (the
    // intrinsics table doesn't carry it; the assign-boundary
    // consumer declares its own).
    let arr_any_to_typed = crate::ssa_lower_synthesize_main::declare_intrinsic(
        module,
        fn_table,
        "__torajs_arr_any_to_typed",
        &[Type::Ptr, Type::I64],
        Type::Ptr,
    );
    // S2.39 — the literal-default substitution kernel rides the
    // intrinsics table since S3.8 (the direct-call terminal consumes
    // it too; a second lazy declare here would push a same-named
    // duplicate declaration into the module).
    let anyv_or_default = intr.anyv_or_default;
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
            t.body_fid,
            &t.user_tys,
            t.ret_ty,
            t.has_argv,
            t.recv_slot,
            t.feeds_env,
            t.hidden_argc,
            &t.dflt_lits,
            t.rest,
            t.rest_kind,
            arr_any_to_typed,
            t.ret_arr_kind,
        );
        entries.insert(t.fid, pair);
    }
    entries
}

mod admit;
pub(crate) use admit::program_constructs_from_value;

mod dflt_lit;
pub(crate) use dflt_lit::{DfltLit, FnDfltLits};

mod build;
use build::build_boxed_entry;
mod relay;
mod targets;
use targets::{TargetTables, collect_boxed_targets};
