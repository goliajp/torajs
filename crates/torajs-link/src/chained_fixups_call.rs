//! SD-4c-prereq+e8 — thin wrapper around `build_chained_fixups` so
//! `compute_archive_layout` can call it without exceeding the 500-
//! prod-LOC file-size hard limit. Carries the data_const seg /
//! rebase-target plumbing the e8 vtable migration introduced.

use std::collections::BTreeMap;

use crate::chained_fixups::{TextRebaseScope, build_chained_fixups};
use crate::chained_fixups_starts::RebaseTarget;
use crate::data_const_layout::DataConstLayout;
use crate::lc::TEXT_VMADDR_BASE;

/// Args bundle for [`compute_chained_fixups_outputs`] — keeps the
/// archive_link call site to a handful of lines while still
/// passing every layout-derived input through explicitly.
pub struct ChainedFixupsInputs<'a> {
    pub dyld_imports: &'a BTreeMap<String, u8>,
    pub data_seg_vmaddr_offset: u64,
    pub data_seg_vmsize: u64,
    pub tlv_thunk_offsets: &'a [u64],
    pub segment_count: u32,
    pub data_seg_idx: u32,
    pub data_const_layout: &'a DataConstLayout,
    pub vtable_rebase_targets: &'a [RebaseTarget],
    /// SD-4c swap-2k chunk 2b — per staticlib-member `__DATA,*`
    /// rebase target list (offset_in_data_segment, target_off_in_image).
    /// Empty slice (default) keeps the binary byte-identical to the
    /// pre-2b path; 2b-4 wires the actual targets from
    /// [`crate::member_data_rebase_layout::compute_member_data_rebase_targets`]
    /// so dyld rebases each member-data vtable slot at image load.
    pub data_seg_rebase_targets: &'a [RebaseTarget],
}

/// Returns `(blob, la_ptr_slot_values, tlv_thunk_link_values,
/// text_rebase_link_values, data_rebase_link_values)`. Short-circuits
/// to five empty vecs when no la-ptr / TLV / vtable rebase / data
/// rebase participants exist.
pub fn compute_chained_fixups_outputs(
    input: ChainedFixupsInputs<'_>,
) -> (Vec<u8>, Vec<u64>, Vec<u64>, Vec<u64>, Vec<u64>) {
    let has_dyld = !input.dyld_imports.is_empty();
    let has_vtable_rebase = !input.vtable_rebase_targets.is_empty();
    let has_data_rebase = !input.data_seg_rebase_targets.is_empty();
    if !has_dyld && !has_vtable_rebase && !has_data_rebase {
        return (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    }
    let text_rebase = if has_vtable_rebase {
        Some(TextRebaseScope {
            // Field names are vestigial — e8+ uses __DATA_CONST.
            text_segment_vmaddr_offset: input.data_const_layout.segment_vmaddr - TEXT_VMADDR_BASE,
            text_segment_vmsize: input.data_const_layout.segment_vmsize,
            text_seg_idx: 2, // __DATA_CONST idx in PAGEZERO+TEXT+DATA_CONST+...
            rebase_targets: input.vtable_rebase_targets,
        })
    } else {
        None
    };
    let built = build_chained_fixups(
        input.dyld_imports,
        input.data_seg_vmaddr_offset,
        0,
        input.data_seg_vmsize,
        input.tlv_thunk_offsets,
        input.segment_count,
        input.data_seg_idx,
        text_rebase.as_ref(),
        input.data_seg_rebase_targets,
    );
    (
        built.blob,
        built.la_ptr_slot_values,
        built.tlv_thunk_link_values,
        built.text_rebase_link_values,
        built.data_rebase_link_values,
    )
}
