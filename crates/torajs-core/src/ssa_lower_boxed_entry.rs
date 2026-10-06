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
use unbox::{BoxedCoerceIntrinsics, declare_coerce_intrinsics};

/// The runtime dispatcher's fixed argv width (undefined-filled).
pub(crate) const MAX_BOXED_PARAMS: usize = 8;

/// Intrinsic FuncIds the adapters call — a narrow slice of
/// `Intrinsics` so Pass 1 doesn't need the whole table.
pub(crate) struct BoxedEntryIntrinsics {
    /// `__torajs_anyv_box_from_pair(tag, value) -> AnyValue`.
    pub(crate) any_box: FuncId,
    /// `__torajs_anyv_to_number(v) -> f64` (ES ToNumber).
    pub(crate) any_to_number: FuncId,
    /// `__torajs_anyv_to_f64_slot(v) -> f64` — ToNumber, except that
    /// `undefined` keeps the F64 sentinel (a `number` parameter slot).
    pub(crate) any_to_f64_slot: FuncId,
    /// `__torajs_anyv_to_bool(v) -> bool` (ES ToBoolean).
    pub(crate) any_to_bool: FuncId,
    /// `__torajs_anyv_unbox_tag(v) -> i64` (5 = undefined) — the
    /// mono adapter's lossy-slot test ahead of its unboxes.
    pub(crate) any_unbox_tag: FuncId,
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
    /// `__torajs_anyv_box_str_slot(s) -> AnyValue` — a Str RETURN
    /// crosses into the any world through the slot's own encoding,
    /// so the undefined sentinel a string slot carries comes out as
    /// `undefined`, not as a heap box of the sentinel cell.
    pub(crate) anyv_box_str_slot: FuncId,
}

/// Walk the lifted closure fns and synthesize one adapter each.
/// Returns `body fid → (adapter fid, adapter sig)` for the closure
/// construction sites.
// the lowering tables it reads and the module tables it extends are separate borrows
#[allow(clippy::too_many_arguments)]
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
    // Classes with the same layout share one sid; the lowest tag
    // answers for it, so the choice never follows hash order.
    let mut named_sid_to_tag: HashMap<ssa::StructId, u32> = HashMap::new();
    for (cname, &tag) in class_name_to_tag {
        if let Some(Type::Obj(sid)) = aliases.get(cname) {
            let t = named_sid_to_tag.entry(*sid).or_insert(tag);
            *t = (*t).min(tag);
        }
    }
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
    let ids = declare_coerce_intrinsics(module, fn_table);
    let (twins, monos): (Vec<_>, Vec<_>) = targets.into_iter().partition(|t| {
        is_twin_name(&t.name) || crate::check_monomorph_any_widen::struct_escape::is_twin(&t.name)
    });
    for t in twins.into_iter().chain(monos) {
        let slots: Vec<SlotTest> = t
            .user_tys
            .iter()
            .map(|ty| slot_test(*ty, arr_layouts, &sid_tag))
            .collect();
        let twin_reroute = twin_adapter_of(ast, fn_table, &entries, &t.name).filter(|_| {
            t.user_tys.iter().enumerate().any(|(i, ty)| {
                lossy_slot(
                    *ty,
                    t.dflt_lits.get(i).and_then(|d| d.as_ref()),
                    slots[i],
                    t.rest && i + 1 == t.user_tys.len(),
                )
            })
        });
        let same_face = env_faced(&t.name);
        let pair = build_boxed_entry(
            module,
            fn_table,
            fn_sigs,
            fn_sig_ids,
            intr,
            ids.anyv_to_str,
            BoxedCoerceIntrinsics {
                ids,
                anyv_or_default: intr.anyv_or_default,
                slots: &slots,
                rest: t.rest,
                twin_reroute: twin_reroute.map(|(adapter, sig)| TwinReroute {
                    adapter,
                    sig,
                    invoke: ids.invoke_recv_first,
                    same_face,
                }),
                fit: vec![None; t.user_tys.len()],
            },
            &t.name,
            t.body_fid,
            &t.user_tys,
            t.ret_ty,
            t.has_argv,
            t.recv_slot,
            t.lead,
            t.lead_undef,
            t.feeds_env,
            t.hidden_argc,
            &t.dflt_lits,
            t.rest,
            t.rest_kind,
            ids.arr_any_to_typed,
            t.ret_arr_kind,
        );
        entries.insert(t.fid, pair);
    }
    entries
}

mod admit;
pub(crate) use admit::{
    factory_always_admitted, program_constructs_from_value, promise_heir_classes,
};

mod dflt_lit;
pub(crate) use dflt_lit::{DfltLit, FnDfltLits};

mod build;
mod build_reroute;
mod build_rest;
mod fit_tag;
mod probe;
use build::build_boxed_entry;
pub(crate) use build_reroute::twin_adapter_of;
use build_reroute::{TwinReroute, is_twin_name};
use fit_tag::{SlotTest, lossy_slot, slot_test};
mod relay;
mod targets;
use targets::{TargetTables, collect_boxed_targets};

/// A mono whose twin wears its own `(env, argv, argc)` face: a relay
/// shell, or a lifted closure.
fn env_faced(name: &str) -> bool {
    crate::ast::is_shell(name) || name.starts_with("__closure_")
}
