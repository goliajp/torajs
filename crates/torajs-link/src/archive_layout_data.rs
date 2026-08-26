//! Phase D of `compute_archive_layout` — `__DATA_CONST` +
//! `__DATA` (la_ptr / member data sections / TLV descriptors /
//! user bss) layout, `__LINKEDIT` placement, and per-import stub
//! vaddrs, split from `archive_link.rs` (2026-07-03, fn-debt
//! decomp). Body verbatim; phase-T inputs read via `tp.*`.

use std::collections::BTreeMap;

use crate::archive_layout_text::TextRegionPlan;
use crate::archive_link::round_up_to;
use crate::archives_merge::{MergedArchives, RequiredMembers};
use crate::data_const_layout::{DataConstLayout, compute_data_const_layout};
use crate::data_section_layout::{DataSectionLayout, compute_data_section_layouts};
use crate::exec::LinkConfig;
use crate::layout_types::ArchiveLayoutError;
use crate::lc::{APPLE_SILICON_PAGE_SIZE, TEXT_VMADDR_BASE};
use crate::non_text_layout::NonTextLayoutError;
use crate::stubs::STUB_SIZE;
use crate::tlv_descriptor_layout::{TlvDescriptorLayout, compute_tlv_descriptor_layouts};
use crate::user_data_globals_layout::{UserDataGlobalsLayout, build_user_data_globals_region};

/// Phase-D outputs consumed by the linkedit phase and the final
/// [`crate::layout_types::ArchiveLayout`] assembly.
pub(crate) struct DataRegionPlan {
    pub(crate) data_const_layout: DataConstLayout,
    pub(crate) has_data_const: bool,
    pub(crate) data_file_offset: u32,
    pub(crate) data_vmaddr: u64,
    pub(crate) stubs_section_vaddr: u64,
    pub(crate) la_ptr_section_vaddr: u64,
    pub(crate) data_non_text_layouts: Vec<Vec<DataSectionLayout>>,
    pub(crate) data_merged_sections: Vec<crate::data_section_layout::MergedDataSection>,
    pub(crate) data_non_text_file_offset: u32,
    pub(crate) data_non_text_file_size: u32,
    pub(crate) data_non_text_zerofill_vmsize: u32,
    pub(crate) tlv_descriptors: Vec<TlvDescriptorLayout>,
    pub(crate) data_filesize: u64,
    pub(crate) user_data_globals_layout: UserDataGlobalsLayout,
    pub(crate) data_vmsize: u64,
    pub(crate) linkedit_file_offset: u32,
    pub(crate) linkedit_vmaddr: u64,
    pub(crate) stub_vaddrs: BTreeMap<String, u64>,
}

pub(crate) fn compute_data_region_plan(
    cfg: &LinkConfig,
    required: &RequiredMembers,
    merged: &MergedArchives<'_>,
    member_keys: &[(usize, usize)],
    tp: &TextRegionPlan,
) -> Result<DataRegionPlan, ArchiveLayoutError> {
    // e8 + W-J A3c __DATA_CONST — vtable + class_layouts +
    // fn_name_table + class_name_table rodata.
    let data_const_layout = compute_data_const_layout(
        &cfg.vtable_globals,
        &cfg.class_layouts,
        cfg.force_emit_class_layouts_globals,
        &cfg.fn_name_globals,
        cfg.force_emit_fn_name_globals,
        &cfg.class_names,
        cfg.force_emit_class_names_globals,
        &cfg.baked_regex_entries,
        tp.text_vmsize as u32,
        TEXT_VMADDR_BASE + tp.text_vmsize,
    );
    let has_data_const = data_const_layout.has_data_const;
    let data_file_offset = if tp.has_dyld {
        data_const_layout.segment_file_offset + data_const_layout.segment_filesize as u32
    } else {
        0
    };
    let data_vmaddr = if tp.has_dyld {
        data_const_layout.segment_vmaddr + data_const_layout.segment_vmsize
    } else {
        0
    };

    // Section vaddrs follow directly from segment vmaddrs +
    // section file offsets.
    let stubs_section_vaddr = if tp.has_dyld {
        TEXT_VMADDR_BASE + u64::from(tp.stubs_file_offset)
    } else {
        0
    };
    let la_ptr_section_vaddr = data_vmaddr;

    // c-fix-c3/c4 — `__DATA,*` past `__la_symbol_ptr` (chain seg_off=0).
    let (
        data_non_text_layouts,
        data_merged_sections,
        data_non_text_file_offset,
        data_non_text_file_size,
        data_non_text_zerofill_vmsize,
    ) = if tp.has_dyld {
        let file_start = data_file_offset + tp.la_ptr_section_size as u32;
        let vaddr_start = data_vmaddr + tp.la_ptr_section_size;
        let res = compute_data_section_layouts(&merged, &member_keys, file_start, vaddr_start)
            .map_err(
                |NonTextLayoutError {
                     archive_idx,
                     member_idx,
                     err,
                 }| {
                    ArchiveLayoutError::MemberSections {
                        archive_idx,
                        member_idx,
                        err,
                    }
                },
            )?;
        (
            res.per_member,
            res.merged,
            file_start,
            res.file_region_size,
            res.zerofill_vmsize,
        )
    } else {
        (Vec::new(), Vec::new(), 0u32, 0u32, 0u32)
    };

    // b1/c — member __DATA,__thread_vars TLV descriptors for chain
    // per-thunk-slot binds. has_dyld=false → no __DATA → no TLVs.
    let tlv_descriptors = if tp.has_dyld {
        compute_tlv_descriptor_layouts(&member_keys, &data_non_text_layouts)
            .expect("TLV descriptor layout cannot fail when inputs are zipped from compute_data_section_layouts")
            .descriptors
    } else {
        Vec::new()
    };

    // __DATA: data_filesize page-aligns for next-seg fileoff; vmsize
    // extends with zerofill (member zerofill + e4 user-bss).
    let data_total_file_size = if tp.has_dyld {
        tp.la_ptr_section_size + u64::from(data_non_text_file_size)
    } else {
        0
    };
    let data_filesize = if tp.has_dyld {
        round_up_to(data_total_file_size, APPLE_SILICON_PAGE_SIZE)
    } else {
        0
    };
    // e4 — `__DATA,__bss` zerofill past member zerofill.
    let user_data_globals_layout = build_user_data_globals_region(
        &cfg.data_globals,
        tp.has_dyld,
        data_vmaddr + data_total_file_size + u64::from(data_non_text_zerofill_vmsize),
    )
    .map_err(|count| ArchiveLayoutError::DataGlobalsWithoutDyld { count })?;
    let data_vmsize = if tp.has_dyld {
        round_up_to(
            data_total_file_size
                + u64::from(data_non_text_zerofill_vmsize)
                + u64::from(user_data_globals_layout.total_vmsize),
            APPLE_SILICON_PAGE_SIZE,
        )
    } else {
        0
    };

    // e8: __DATA_CONST seg sits between __TEXT and __DATA; account
    // for its file + vm footprint when locating __LINKEDIT.
    let linkedit_file_offset =
        (tp.text_vmsize + data_const_layout.segment_filesize + data_filesize) as u32;
    let linkedit_vmaddr =
        TEXT_VMADDR_BASE + tp.text_vmsize + data_const_layout.segment_vmsize + data_vmsize;

    // Per-import stub vaddr stride is STUB_SIZE. BTreeMap iter is
    // sorted-by-name → stub_vaddrs key order is reproducible.
    let mut stub_vaddrs: BTreeMap<String, u64> = BTreeMap::new();
    if tp.has_dyld {
        for (i, name) in required.dyld_imports.keys().enumerate() {
            stub_vaddrs.insert(name.clone(), stubs_section_vaddr + (i as u64) * STUB_SIZE);
        }
    }
    Ok(DataRegionPlan {
        data_const_layout,
        has_data_const,
        data_file_offset,
        data_vmaddr,
        stubs_section_vaddr,
        la_ptr_section_vaddr,
        data_non_text_layouts,
        data_merged_sections,
        data_non_text_file_offset,
        data_non_text_file_size,
        data_non_text_zerofill_vmsize,
        tlv_descriptors,
        data_filesize,
        user_data_globals_layout,
        data_vmsize,
        linkedit_file_offset,
        linkedit_vmaddr,
        stub_vaddrs,
    })
}
