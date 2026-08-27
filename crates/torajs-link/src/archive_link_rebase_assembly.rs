//! W-J Phase A3c chunk 2 — `compute_archive_layout` Phase 3 helper:
//! assemble the combined `__DATA_CONST` text-rebase target list +
//! per-region counts in one call.
//!
//! Order matters: dyld walks slots in
//! `class_layouts | fn_name_table | class_name_table | baked_regex`
//! concatenation order so the per-slot link values come back in
//! the same order. archive_emit.rs slices that vec at the recorded
//! counts (`fn_name_rebase_target_count` +
//! `class_name_rebase_target_count` + `baked_regex_rebase_target_count`;
//! class_layouts is the head remainder) to dispatch each region's
//! payload builder. Vtables are relative since r506 and never enter
//! this list.

use crate::chained_fixups_starts::RebaseTarget;
use crate::class_name_table_layout::class_name_table_rebase_targets_from_layouts;
use crate::data_const_layout::DataConstLayout;
use crate::fn_name_table_layout::fn_name_table_rebase_targets_from_layouts;
use crate::lc::TEXT_VMADDR_BASE;
use crate::user_class_layouts_layout::compute_class_layouts_rebase_targets;
use crate::user_regex_baked_layout::compute_user_regex_baked_rebase_targets;
use crate::user_strings_layout::UserStringsLayout;

/// Combined rebase output for `compute_archive_layout` Phase 3.
pub struct TextRebaseAssembly {
    pub combined: Vec<RebaseTarget>,
    pub fn_name_rebase_target_count: usize,
    pub class_name_rebase_target_count: usize,
    /// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-5c.2b — count of
    /// `__torajs_baked_regex_<i>::states_ptr` slots appended after
    /// the class_name region. Zero in current state since the
    /// SSA-side `Module` doesn't push entries until C-6 trips the
    /// AOT gate. emit-pass `data_const_payload` writer will slice
    /// link-values past `class_layouts + fn_name + class_name` to
    /// drive the baked-regex region's rebase fill.
    pub baked_regex_rebase_target_count: usize,
}

pub fn assemble_text_rebase_targets(
    data_const_layout: &DataConstLayout,
    fn_vaddrs: &[u64],
    user_strings_layout: &UserStringsLayout,
) -> TextRebaseAssembly {
    let seg_vmaddr_base = data_const_layout.segment_vmaddr;
    let class_layouts_targets = compute_class_layouts_rebase_targets(
        &data_const_layout.class_layouts_layout,
        fn_vaddrs,
        seg_vmaddr_base,
        TEXT_VMADDR_BASE,
    );
    let fn_name_targets = fn_name_table_rebase_targets_from_layouts(
        &data_const_layout.fn_name_table_layout,
        fn_vaddrs,
        user_strings_layout,
        seg_vmaddr_base,
        TEXT_VMADDR_BASE,
    );
    let class_name_targets = class_name_table_rebase_targets_from_layouts(
        &data_const_layout.class_name_table_layout,
        user_strings_layout,
        seg_vmaddr_base,
        TEXT_VMADDR_BASE,
    );
    let baked_regex_targets = compute_user_regex_baked_rebase_targets(
        &data_const_layout.baked_regex_layout,
        seg_vmaddr_base,
        TEXT_VMADDR_BASE,
    );
    let fn_name_rebase_target_count = fn_name_targets.len();
    let class_name_rebase_target_count = class_name_targets.len();
    let baked_regex_rebase_target_count = baked_regex_targets.len();
    let mut combined = class_layouts_targets;
    combined.extend(fn_name_targets);
    combined.extend(class_name_targets);
    combined.extend(baked_regex_targets);
    TextRebaseAssembly {
        combined,
        fn_name_rebase_target_count,
        class_name_rebase_target_count,
        baked_regex_rebase_target_count,
    }
}
