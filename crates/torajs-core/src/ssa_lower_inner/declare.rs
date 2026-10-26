//! Pass 0 of `lower_inner`: the four intrinsic-declare batches.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module};

/// M2.A fix — lifted closures (`__closure_N`) must lower in REVERSE
/// append order so each closure's CONSTRUCTION site (in its enclosing
/// fn / outer closure) runs before its BODY (which reads
/// `closure_captures` populated by the construction). Without this
/// reorder, nested capturing closures crashed: __closure_0 (innermost)
/// is appended first by lift_arrow_fns and would lower first, but its
/// captures are populated by __closure_1 (outer)'s body lowering.
///
/// Pass 0 — the four intrinsic-declare batches (A/B/C/D aggregator
/// siblings). Split 2026-07-03 (fn-debt decomp); bodies verbatim.
pub(super) fn declare_intrinsic_batches(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> (
    crate::ssa_lower_intrinsics_init_a::InitA,
    crate::ssa_lower_intrinsics_init_b::InitB,
    crate::ssa_lower_intrinsics_init_c::InitC,
    crate::ssa_lower_intrinsics_init_d::InitD,
) {
    // Pass 0 batch A — print / obj_capture / arr / str_a / num / str_b
    // (6 sub-systems, 76 FuncIds) declare via aggregator in
    // [`crate::ssa_lower_intrinsics_init_a`] (chunk-323 of the
    // ssa_lower.rs god-file + lower_inner god-fn decomp). The
    // `init_a.<group>.<field>` references appear directly in the
    // `Intrinsics { ... }` literal below; no local *_id vars are
    // needed because nothing between here and the literal reads them.
    // the batches declare about a thousand names: room for them up front
    // rather than a rehash at every doubling on the way
    fn_table.reserve(1 << 10);
    let init_a = crate::ssa_lower_intrinsics_init_a::declare(module, fn_table);
    // Pass 0 batch B — regex / date / fs / process / arr_any / object
    // (6 sub-systems, 121 FuncIds) declare via aggregator in
    // [`crate::ssa_lower_intrinsics_init_b`] (chunk-324 RFC continuation).
    let init_b = crate::ssa_lower_intrinsics_init_b::declare(module, fn_table);
    // Pass 0 batch C — any_substrate / print_freeze / bigint / weak /
    // map_set / runtime_misc (6 sub-systems, 123 FuncIds) declare via
    // aggregator in [`crate::ssa_lower_intrinsics_init_c`] (chunk-325
    // RFC continuation). Sibling also folds in the `gc` alias insert
    // and the `main_exit` / `process_on` declares that immediately
    // followed runtime_misc in the original Pass 0.
    let init_c = crate::ssa_lower_intrinsics_init_c::declare(module, fn_table);
    // Pass 0 batch D — promise / substr / substr_trim_into /
    // arr_str_etc / str_extra / math / json_misc / throw (8
    // sub-systems, 170 FuncIds) declare via aggregator in
    // [`crate::ssa_lower_intrinsics_init_d`] (chunk-326 RFC final
    // batch). After this Pass 0 is fully drained from `lower_inner`.
    let init_d = crate::ssa_lower_intrinsics_init_d::declare(module, fn_table);
    crate::ssa_lower_member_obj_field::declare_deletable_accessor(module, fn_table);
    crate::ssa_lower_call_promise_static::declare_all_sync_proved(module, fn_table);
    (init_a, init_b, init_c, init_d)
}
