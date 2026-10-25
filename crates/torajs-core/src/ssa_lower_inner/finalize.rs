//! The module's last step in `lower_inner`: interner write-backs and
//! the vtable / class layout metadata. Moved out of the parent whole.

use std::collections::HashMap;

use crate::ast::{Ast, PropKey};
use crate::ssa::{self, FuncId, Module, Type};

/// Interner write-backs + vtable / ClassLayoutMeta emit. Split
/// 2026-07-03 (fn-debt decomp); body verbatim.
#[allow(clippy::too_many_arguments)]
pub(super) fn finalize_module(
    module: &mut Module,
    ast: &Ast,
    fn_table: &HashMap<String, FuncId>,
    fn_sig_ids: &HashMap<FuncId, ssa::SigId>,
    boxed_entries: &HashMap<FuncId, (FuncId, ssa::SigId)>,
    arr_layouts: crate::ssa::ArrLayouts,
    fn_sigs: Vec<(Vec<Type>, Type)>,
    struct_layouts: Vec<Vec<(PropKey, Type)>>,
    baked_regex_buf: Vec<ssa::BakedRegexEntry>,
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, Type>,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    struct_layouts_pass15_len: usize,
) {
    module.arr_layouts = arr_layouts;
    module.signatures = fn_sigs;
    module.struct_layouts = struct_layouts;
    module.baked_regex_entries = baked_regex_buf;

    // T-24 vtables (per-class slot dispatch) + T-26.C named-class
    // ClassLayoutMeta + W-J Phase A0 anonymous-struct ClassLayoutMeta
    // — see [`crate::ssa_lower_module_metadata`] for the consolidated
    // builder docs.
    crate::ssa_lower_module_metadata_vtables::populate_vtables(ast, fn_table, module);
    // 507-02 — one slot, one ABI: the indirect call through a slot
    // takes ITS signature from a single owner, so the table is only
    // callable if every row agrees. Nothing else pins that; the
    // union-find that establishes it (`num_width::slot_abi`) is a
    // different pass with its own idea of which bodies share a slot.
    crate::ssa_lower_module_metadata_slot_abi::assert_vtable_slot_abi(ast, module, fn_sig_ids);
    let generic_methods = crate::ssa_lower_module_metadata::populate_class_layouts(
        ast,
        fn_table,
        boxed_entries,
        class_name_to_tag,
        aliases,
        module,
        struct_layouts_pass15_len,
    );

    // W-J Phase A1 follow-up — append `ClassLayoutMeta` rows for
    // each Pass 2 fresh tag recorded in `anon_stamp_pool` (plain
    // anon sids + the 405-03 generic-factory rows, merged by tag).
    crate::ssa_lower_anon_stamp::append_fresh_class_layouts(
        anon_stamp_pool,
        &generic_methods,
        class_name_to_tag,
        module,
    );
}
