//! Phase L of `compute_archive_layout` — text-rebase assembly,
//! chained-fixups blob, `__LINKEDIT` data offsets (chain blob +
//! codesign) and total size, split from `archive_link.rs`
//! (2026-07-03, fn-debt decomp). Body verbatim; phase-T / phase-D
//! inputs read via `tp.*` / `dp.*`.

use crate::archive_layout_data::DataRegionPlan;
use crate::archive_layout_text::TextRegionPlan;
use crate::archive_link::round_up_to;
use crate::archive_link_rebase_assembly::{TextRebaseAssembly, assemble_text_rebase_targets};
use crate::archives_merge::RequiredMembers;
use crate::chained_fixups_call::{ChainedFixupsInputs, compute_chained_fixups_outputs};
use crate::exec::LinkConfig;
use crate::lc::{APPLE_SILICON_PAGE_SIZE, TEXT_VMADDR_BASE};
use crate::sign::adhoc_codesign_blob_size;

/// Phase-L outputs consumed by the final
/// [`crate::layout_types::ArchiveLayout`] assembly.
pub(crate) struct LinkeditPlan {
    pub(crate) chained_fixups_blob: Vec<u8>,
    pub(crate) la_ptr_slot_values: Vec<u64>,
    pub(crate) tlv_thunk_link_values: Vec<u64>,
    pub(crate) text_rebase_link_values: Vec<u64>,
    pub(crate) vtable_rebase_target_count: usize,
    pub(crate) fn_name_rebase_target_count: usize,
    pub(crate) class_name_rebase_target_count: usize,
    pub(crate) baked_regex_rebase_target_count: usize,
    pub(crate) chained_fixups_dataoff: u32,
    pub(crate) chained_fixups_datasize: u32,
    pub(crate) codesign_dataoff: u32,
    pub(crate) codesign_datasize: u32,
    pub(crate) linkedit_vmsize: u64,
    pub(crate) total_size: u32,
}

pub(crate) fn compute_linkedit_plan(
    cfg: &LinkConfig,
    required: &RequiredMembers,
    tp: &TextRegionPlan,
    dp: &DataRegionPlan,
    fn_vaddrs: &[u64],
    stroff: u32,
    strsize: u32,
) -> LinkeditPlan {
    // SD-3 chain blob; +c binds each TLV thunk to `__tlv_bootstrap`.
    let tlv_thunk_offsets: Vec<u64> = dp
        .tlv_descriptors
        .iter()
        .map(|d| d.thunk_slot_vaddr - dp.data_vmaddr)
        .collect();
    // Phase 3 — concat vtable | class_layouts | fn_name_table |
    // class_name_table rebase in walk order so dyld + archive_emit
    // can per-region split via the recorded counts.
    let TextRebaseAssembly {
        combined: combined_text_rebase_targets,
        vtable_rebase_target_count,
        fn_name_rebase_target_count,
        class_name_rebase_target_count,
        baked_regex_rebase_target_count,
    } = assemble_text_rebase_targets(&dp.data_const_layout, fn_vaddrs, &tp.user_strings_layout);

    // e8: __DATA_CONST idx=2; __DATA shifts to idx 3 when has_data_const.
    let data_seg_idx: u32 = if dp.has_data_const { 3 } else { 2 };
    let (
        chained_fixups_blob,
        la_ptr_slot_values,
        tlv_thunk_link_values,
        text_rebase_link_values,
        _data_rebase_link_values,
    ) = compute_chained_fixups_outputs(ChainedFixupsInputs {
        lc_present: tp.has_chained_fixups,
        dyld_imports: &required.dyld_imports,
        data_seg_vmaddr_offset: dp.data_vmaddr.saturating_sub(TEXT_VMADDR_BASE),
        data_seg_filesize: dp.data_filesize,
        tlv_thunk_offsets: &tlv_thunk_offsets,
        segment_count: tp.segment_count,
        data_seg_idx,
        data_const_layout: &dp.data_const_layout,
        vtable_rebase_targets: &combined_text_rebase_targets,
        data_seg_rebase_targets: &[],
    });
    // +c: chain + CodeDir both need 8-aligned LINKEDIT offsets.
    let chained_fixups_dataoff = if tp.has_chained_fixups {
        round_up_to(u64::from(stroff + strsize), 8) as u32
    } else {
        0
    };
    let chained_fixups_datasize = chained_fixups_blob.len() as u32;
    let codesign_dataoff = if tp.has_chained_fixups {
        round_up_to(
            u64::from(chained_fixups_dataoff + chained_fixups_datasize),
            8,
        ) as u32
    } else {
        stroff + strsize
    };
    let codesign_datasize = adhoc_codesign_blob_size(codesign_dataoff, &cfg.codesign_ident);
    // __LINKEDIT filesize = codesign end - segment start (covers
    // the alignment pads above).
    let linkedit_data_size =
        (codesign_dataoff + codesign_datasize).saturating_sub(dp.linkedit_file_offset);
    let linkedit_vmsize = round_up_to(u64::from(linkedit_data_size), APPLE_SILICON_PAGE_SIZE);
    let total_size = dp.linkedit_file_offset + linkedit_data_size;
    LinkeditPlan {
        chained_fixups_blob,
        la_ptr_slot_values,
        tlv_thunk_link_values,
        text_rebase_link_values,
        vtable_rebase_target_count,
        fn_name_rebase_target_count,
        class_name_rebase_target_count,
        baked_regex_rebase_target_count,
        chained_fixups_dataoff,
        chained_fixups_datasize,
        codesign_dataoff,
        codesign_datasize,
        linkedit_vmsize,
        total_size,
    }
}
