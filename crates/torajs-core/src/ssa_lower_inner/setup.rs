//! Pass-1.5 boundary helpers split from `ssa_lower_inner.rs` — the
//! `Intrinsics` dispatch-table + boxed-adapter synthesize pair and the
//! `anon_stamp` snapshot builder, both consumed exactly once at the
//! seam between Pass 1 (fn/global registration) and Pass 2 (body
//! lowering). Extracted as the rotation-197 file-size sweep to keep
//! `ssa_lower_inner.rs` under the 500-line hard cap after rotation
//! 197 chunk 2 grew the parent past it. Verbatim moves.

use std::collections::HashMap;

use crate::ast::Ast;
use crate::ssa::{FuncId, Module};

/// Pass-1.5 boundary — build the per-module `Intrinsics` dispatch table
/// then synthesize the boxed dual-entry adapters for every lifted-closure
/// / class-method body.
///
/// C3a-2 (RFC 20260704-any-method-call) — synthesize the boxed
/// dual-entry adapter per lifted closure body. Same freeze-point
/// constraint as the promise thunks: the module fn list must not
/// grow once per-fn lowering starts. Adapters never appear as
/// static call sites, so the `signatures` snapshot above staying
/// adapter-free is fine (their FnAddr refs carry their own sig).
pub(super) fn build_intrinsics_and_boxed_entries(
    ast: &Ast,
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<crate::ssa::Type>, crate::ssa::Type)>,
    fn_sig_ids: &mut HashMap<FuncId, crate::ssa::SigId>,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, crate::ssa::Type>,
    env_drop_trivial_fid: (FuncId, crate::ssa::SigId),
    init_a: &crate::ssa_lower_intrinsics_init_a::InitA,
    init_b: &crate::ssa_lower_intrinsics_init_b::InitB,
    init_c: &crate::ssa_lower_intrinsics_init_c::InitC,
    init_d: &crate::ssa_lower_intrinsics_init_d::InitD,
) -> (
    crate::ssa_lower::Intrinsics,
    HashMap<FuncId, (FuncId, crate::ssa::SigId)>,
) {
    let intrinsics = crate::ssa_lower_intrinsics_table::build(
        env_drop_trivial_fid,
        init_a,
        init_b,
        init_c,
        init_d,
    );
    let boxed_entries = crate::ssa_lower_boxed_entry::synthesize_boxed_entries(
        ast,
        module,
        fn_table,
        fn_sigs,
        fn_sig_ids,
        &crate::ssa_lower_boxed_entry::BoxedEntryIntrinsics {
            any_box: intrinsics.any_box,
            any_to_number: intrinsics.any_to_number,
            any_to_bool: intrinsics.any_to_bool,
            any_unbox_value: intrinsics.any_unbox_value,
            str_drop: intrinsics.str_drop,
            arr_alloc_any: intrinsics.arr_alloc_any,
            arr_any_push: intrinsics.arr_any_push,
            anyv_or_default: intrinsics.anyv_or_default,
        },
        anon_stamp_pool,
        class_name_to_tag,
        aliases,
    );
    (intrinsics, boxed_entries)
}

/// W-J Phase A1 (RFC 20260614-w-j-struct-reflect §3) — anon_sid_to_tag
/// snapshot for ObjectLit alloc stamping. Build at Pass 1.5 boundary
/// (just after collect_toplevel_globals) so the Pass 2 lowerers can
/// stamp `class_tag@+8` on anonymous ObjectLit allocations via the map.
///
/// MVP scope: sids visible at snapshot time get their stamp; sids
/// interned later inside Pass 2 (e.g. generic mono spawning a fresh
/// `{a:1}` shape per specialization) fall back to `unwrap_or(0)` and
/// stay anon-untagged. The downstream reflection consumers (Phase B+)
/// see those as "no struct layout" — graceful degradation, not a crash.
///
/// Tag space layout (must align with the class_layouts emit loop's
/// push order — A0 + this anon push both walk `struct_layouts.iter()
/// .enumerate().filter(|(idx,_)| !named_sids.contains(idx))`):
///   - [1 .. n_named]                                = named classes
///   - [n_named+1 .. n_named+n_anon] (per anon_idx)  = anonymous structs
/// Cycle visitor indexes class_layouts via `class_tag - 1`, so the
/// vec push order must mirror this enumeration.
///
/// P1 rc bug 2026-07-23 fix — also return the Pass-1.5 snapshot boundary
/// so populate_class_layouts stays in sync with pool tag assignments.
/// Every sid appended past this index is Pass-2 territory: either fresh
/// (pool-assigned tag via append) or pool-agnostic (e.g. iter_next's
/// IteratorResult hardcodes class_tag=0, never looked up in
/// class_layouts). Emitting either shape in populate shifts anon indices
/// and breaks the pool's `next_tag_start = n_named + snapshot.len() + 1`
/// invariant.
pub(super) fn build_anon_stamp_pool_with_snapshot(
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, crate::ssa::Type>,
    struct_layouts: &[Vec<(String, crate::ssa::Type)>],
) -> (crate::ssa_lower_anon_stamp::AnonStampPoolCell, usize) {
    let anon_stamp_pool = crate::ssa_lower_anon_stamp::build_snapshot_pool(
        class_name_to_tag,
        aliases,
        struct_layouts,
    );
    let struct_layouts_pass15_len = struct_layouts.len();
    (anon_stamp_pool, struct_layouts_pass15_len)
}
