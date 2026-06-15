//! W-J Phase A3c chunk 2 — `compute_archive_layout` Phase 3 helper:
//! assemble the combined `__DATA_CONST` text-rebase target list +
//! per-region counts in one call.
//!
//! Order matters: dyld walks slots in
//! `vtable | class_layouts | fn_name_table | class_name_table`
//! concatenation order so the per-slot link values come back in
//! the same order. archive_emit.rs slices that vec at the recorded
//! counts (`vtable_rebase_target_count` + `fn_name_rebase_target_count`
//! + `class_name_rebase_target_count`) to dispatch each region's
//! payload builder.

use crate::chained_fixups_starts::RebaseTarget;
use crate::class_name_table_layout::class_name_table_rebase_targets_from_layouts;
use crate::data_const_layout::DataConstLayout;
use crate::fn_name_table_layout::fn_name_table_rebase_targets_from_layouts;
use crate::lc::TEXT_VMADDR_BASE;
use crate::user_class_layouts_layout::compute_class_layouts_rebase_targets;
use crate::user_strings_layout::UserStringsLayout;
use crate::user_vtables_layout::vtable_rebase_targets_from_fn_vaddrs;

/// Combined rebase output for `compute_archive_layout` Phase 3.
pub struct TextRebaseAssembly {
    pub combined: Vec<RebaseTarget>,
    pub vtable_rebase_target_count: usize,
    pub fn_name_rebase_target_count: usize,
    pub class_name_rebase_target_count: usize,
}

pub fn assemble_text_rebase_targets(
    data_const_layout: &DataConstLayout,
    fn_vaddrs: &[u64],
    user_strings_layout: &UserStringsLayout,
) -> TextRebaseAssembly {
    let seg_vmaddr_base = data_const_layout.segment_vmaddr;
    let vtable_targets = vtable_rebase_targets_from_fn_vaddrs(
        &data_const_layout.vtable_layout,
        fn_vaddrs,
        seg_vmaddr_base,
        TEXT_VMADDR_BASE,
    );
    let class_layouts_targets = compute_class_layouts_rebase_targets(
        &data_const_layout.class_layouts_layout,
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
    let vtable_rebase_target_count = vtable_targets.len();
    let fn_name_rebase_target_count = fn_name_targets.len();
    let class_name_rebase_target_count = class_name_targets.len();
    let mut combined = vtable_targets;
    combined.extend(class_layouts_targets);
    combined.extend(fn_name_targets);
    combined.extend(class_name_targets);
    TextRebaseAssembly {
        combined,
        vtable_rebase_target_count,
        fn_name_rebase_target_count,
        class_name_rebase_target_count,
    }
}
