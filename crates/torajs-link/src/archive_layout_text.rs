//! Phase T of `compute_archive_layout` — header/LC sizing flags +
//! `__TEXT` region layout (user fns / member `__text`s / member
//! non-text / user strings / stubs), split from `archive_link.rs`
//! (2026-07-03, fn-debt decomp). Body verbatim from the pre-split
//! fn; outputs travel in [`TextRegionPlan`].

use torajs_obj::{MachHeader64, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE, SYMTAB_COMMAND_SIZE};

use crate::archive_link::round_up_to;
use crate::archives_merge::{MergedArchives, RequiredMembers};
use crate::exec::LinkConfig;
use crate::layout_types::ArchiveLayoutError;
use crate::lc::{
    APPLE_SILICON_PAGE_SIZE, BUILD_VERSION_CMDSIZE, DYSYMTAB_CMDSIZE, LINKEDIT_DATA_CMDSIZE,
    LOAD_DYLIB_LIBCURL_CMDSIZE, LOAD_DYLIB_LIBSYSTEM_CMDSIZE, LOAD_DYLINKER_CMDSIZE, MAIN_CMDSIZE,
    TEXT_VMADDR_BASE,
};
use crate::non_text_layout::{NonTextLayoutError, NonTextSectionLayout, compute_non_text_layouts};
use crate::stubs::{LA_PTR_SLOT_SIZE, STUB_SIZE};
use crate::user_strings_layout::{UserStringsLayout, build_user_strings_region};

/// Phase-T outputs consumed by the data / linkedit phases and the
/// final [`crate::layout_types::ArchiveLayout`] assembly.
pub(crate) struct TextRegionPlan {
    pub(crate) has_dyld: bool,
    pub(crate) has_chained_fixups: bool,
    pub(crate) segment_count: u32,
    pub(crate) text_file_offset: u32,
    pub(crate) non_text_region_file_offset: u32,
    pub(crate) non_text_per_member: Vec<Vec<NonTextSectionLayout>>,
    pub(crate) non_text_region_size: u32,
    pub(crate) user_strings_layout: UserStringsLayout,
    pub(crate) user_strings_payload: Vec<u8>,
    pub(crate) text_size: u32,
    pub(crate) stubs_section_size: u64,
    pub(crate) la_ptr_section_size: u64,
    pub(crate) stubs_file_offset: u32,
    pub(crate) text_vmsize: u64,
}

pub(crate) fn compute_text_region_plan(
    cfg: &LinkConfig,
    required: &RequiredMembers,
    merged: &MergedArchives<'_>,
    member_keys: &[(usize, usize)],
    member_text_sizes: &[u32],
) -> Result<TextRegionPlan, ArchiveLayoutError> {
    // Phase 3: layout. has_dyld → __stubs/__la_symbol_ptr/chain LC
    // (libSystem ord 1, libcurl ord 2). e8: __DATA_CONST hosts vtable
    // + class_layouts; chain LC also turns on for __DATA_CONST rebases.
    // Zero-import programs with data globals keep the dyld shape —
    // see `ArchiveLayout::has_dyld` for the platform-contract note.
    let has_dyld = !required.dyld_imports.is_empty() || !cfg.data_globals.is_empty();
    let has_data_const_seg = !cfg.vtable_globals.is_empty()
        || !cfg.class_layouts.is_empty()
        || cfg.force_emit_class_layouts_globals
        || !cfg.fn_name_globals.is_empty()
        || cfg.force_emit_fn_name_globals;
    let has_vtable_rebase = cfg
        .vtable_globals
        .iter()
        .any(|v| v.slot_syms.iter().any(|s| s.is_some()));
    let has_class_layouts_rebase = cfg
        .class_layouts
        .iter()
        .any(|e| !e.child_offsets.is_empty());
    // Step 3b.4 — each fn_name_table entry contributes 2 chain-fixup
    // slots (fn_addr + name_ptr) so any non-empty fn_name_globals
    // triggers the chain pipeline even on otherwise vtable-free
    // programs (e.g. plain `function foo() {}` user TS).
    let has_fn_name_table_rebase =
        !cfg.fn_name_globals.is_empty() || cfg.force_emit_fn_name_globals;
    let has_class_names_table_rebase =
        !cfg.class_names.is_empty() || cfg.force_emit_class_names_globals;
    let has_chained_fixups = has_dyld
        || has_vtable_rebase
        || has_class_layouts_rebase
        || has_fn_name_table_rebase
        || has_class_names_table_rebase;
    let has_libcurl_lc = required.dyld_imports.values().any(|&o| o == 2);
    let load_dylib_size = if has_dyld {
        LOAD_DYLIB_LIBSYSTEM_CMDSIZE
            + if has_libcurl_lc {
                LOAD_DYLIB_LIBCURL_CMDSIZE
            } else {
                0
            }
    } else {
        0
    };
    // segment_count = PAGEZERO + __TEXT + (__DATA_CONST?) + (__DATA?) +
    // __LINKEDIT. section_count = __text + (__stubs? when has_dyld);
    // __DATA_CONST = segment-only rodata blob (no section_64 entry).
    let segment_count = 3 + u32::from(has_dyld) + u32::from(has_data_const_seg);
    // Every section_64 header the emit will write must be counted
    // here: `text_file_offset` is this sum rounded up to a page, and
    // an undercount only survives while the real load-command region
    // happens to fit in the same page. It did not stay fitting — new
    // `__DATA,*` content pushed the emit past the boundary, the whole
    // `__text` payload shifted 16 bytes while every recorded address
    // stayed put, and the entrypoint executed the tail of the
    // preceding function's epilogue: popped argc off the start-up
    // stack into (fp, lr) and ret'd to PC=1. The member data-section
    // count REUSES the data phase's own walk (`count_data_section_64s`)
    // and the user-globals presence test reuses its layout fn, so the
    // sizing here and the emit cannot drift apart again; the emit-side
    // hard gate in `emit_binary` backstops both.
    let data_section_count = if has_dyld {
        crate::data_section_layout::count_data_section_64s(merged, member_keys).map_err(
            |crate::non_text_layout::NonTextLayoutError {
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
        )? + u32::from(
            crate::user_data_globals_layout::compute_user_data_globals_layout(&cfg.data_globals, 0)
                .total_vmsize
                > 0,
        )
    } else {
        0
    };
    let section_count = 1 + if has_dyld { 2 } else { 0 } + data_section_count;
    let chained_fixups_lc_size = if has_chained_fixups {
        LINKEDIT_DATA_CMDSIZE
    } else {
        0
    };
    let sizeofcmds = (SEGMENT_COMMAND_64_SIZE * segment_count)
        + (SECTION_64_SIZE * section_count)
        + LOAD_DYLINKER_CMDSIZE
        + load_dylib_size
        + BUILD_VERSION_CMDSIZE
        + MAIN_CMDSIZE
        + SYMTAB_COMMAND_SIZE
        + DYSYMTAB_CMDSIZE
        + chained_fixups_lc_size
        + LINKEDIT_DATA_CMDSIZE;
    let header_plus_lc = (MachHeader64::SIZE as u32) + sizeofcmds;
    let text_file_offset = round_up_to(u64::from(header_plus_lc), APPLE_SILICON_PAGE_SIZE) as u32;

    let user_text_size: u32 = cfg.funcs.iter().map(|f| f.bytes.len() as u32).sum();
    let members_text_total: u32 = member_text_sizes.iter().copied().sum();

    // Non-text region (member __cstring/__const) lands between member
    // __texts and `__TEXT,__stubs`; folds into text_size auto-shift.
    let non_text_region_file_offset = text_file_offset + user_text_size + members_text_total;
    let non_text_result =
        compute_non_text_layouts(&merged, &member_keys, non_text_region_file_offset).map_err(
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
    let non_text_region_size = non_text_result.region_size;

    // e1 — user strings in `__TEXT,__cstring` past member non-text.
    let (user_strings_layout, user_strings_payload) = build_user_strings_region(
        &cfg.strings,
        TEXT_VMADDR_BASE,
        non_text_region_file_offset + non_text_region_size,
    );
    let text_size =
        user_text_size + members_text_total + non_text_region_size + user_strings_layout.total_size;

    // __TEXT = __text + __stubs (has_dyld); vmsize page-aligned.
    let dyld_count = required.dyld_imports.len() as u64;
    let stubs_section_size = if has_dyld { dyld_count * STUB_SIZE } else { 0 };
    let la_ptr_section_size = if has_dyld {
        dyld_count * LA_PTR_SLOT_SIZE
    } else {
        0
    };
    // chunk 3 — 4-align `__stubs` (ARM instr) past non-text padding.
    let stubs_file_offset = round_up_to(u64::from(text_file_offset + text_size), 4) as u32;
    let text_segment_file_end = u64::from(stubs_file_offset) + stubs_section_size;
    let text_vmsize = round_up_to(text_segment_file_end, APPLE_SILICON_PAGE_SIZE);
    Ok(TextRegionPlan {
        has_dyld,
        has_chained_fixups,
        segment_count,
        text_file_offset,
        non_text_region_file_offset,
        non_text_per_member: non_text_result.per_member,
        non_text_region_size,
        user_strings_layout,
        user_strings_payload,
        text_size,
        stubs_section_size,
        la_ptr_section_size,
        stubs_file_offset,
        text_vmsize,
    })
}
