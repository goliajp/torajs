//! Phase T of `compute_archive_layout` — header/LC sizing flags +
//! `__TEXT` region layout (member `__text`s / user fns / member
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
use crate::text_payload::{TEXT_FN_ALIGN, TextPlacement, place_text, text_slot};
use crate::user_strings_layout::{UserStringsLayout, build_user_strings_region};
use crate::user_vtables_layout::{UserVtablesLayout, build_user_vtables_region};

/// Phase-T outputs consumed by the data / linkedit phases and the
/// final [`crate::layout_types::ArchiveLayout`] assembly.
pub(crate) struct TextRegionPlan {
    pub(crate) has_dyld: bool,
    pub(crate) has_chained_fixups: bool,
    pub(crate) segment_count: u32,
    pub(crate) text_file_offset: u32,
    /// Where each user function and member `__text` starts.
    pub(crate) text_placement: TextPlacement,
    pub(crate) non_text_region_file_offset: u32,
    pub(crate) non_text_per_member: Vec<Vec<NonTextSectionLayout>>,
    pub(crate) non_text_region_size: u32,
    pub(crate) user_strings_layout: UserStringsLayout,
    pub(crate) user_strings_payload: Vec<u8>,
    /// r506 — relative `__vtable_<C>` tables, right after the user
    /// strings inside `__TEXT` (no fixup, no `__DATA_CONST` page).
    pub(crate) user_vtables_layout: UserVtablesLayout,
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
    // (libSystem ord 1, libcurl ord 2). e8: __DATA_CONST hosts the
    // absolute-pointer tables (class_layouts / name tables); chain LC
    // also turns on for __DATA_CONST rebases. Vtables are relative
    // (r506) and live in __TEXT — no chain participation.
    // Zero-import programs with data globals keep the dyld shape —
    // see `ArchiveLayout::has_dyld` for the platform-contract note.
    // A required member's `__text` may reference its OWN `__DATA,*`
    // locals (rustc's alloc rcgu points at `l_anon.*` handler tables
    // in `__DATA,__const`), so the presence of any member data
    // section forces the full data-segment shape regardless of the
    // import / user-global face. Latent until a program's closure
    // carried alloc without any dyld import (the injection-
    // reachability empty shape, RFC 20260825): has_dyld=false
    // skipped the data layout entirely and the member's local reloc
    // resolved against an empty table (UnresolvedSymbol l_anon.*).
    let member_data_section_count =
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
        )?;
    let has_dyld = !required.dyld_imports.is_empty()
        || !cfg.data_globals.is_empty()
        || member_data_section_count > 0;
    let has_data_const_seg = crate::data_const_layout::data_const_present(cfg);
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
        member_data_section_count
            + u32::from(
                crate::user_data_globals_layout::compute_user_data_globals_layout(
                    &cfg.data_globals,
                    0,
                )
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
    // `__text` packs right after the load commands (16-aligned) the
    // way ld64 lays out a hello-world (`__text` at 0x3f84, same page
    // as the header) — the whole `__TEXT` segment maps r-x, header
    // included, so a page boundary here bought nothing and cost one
    // 16 KB page per artifact (r498). An LC-sizing undercount now
    // shifts `__text` immediately instead of hiding until the region
    // outgrows its page; `emit_binary`'s overrun gate catches it.
    let (text_file_offset, text_placement) =
        place_shifted_text(cfg, header_plus_lc, member_text_sizes);

    // Non-text region (member __cstring/__const) lands between the
    // last user fn and `__TEXT,__stubs`; folds into text_size auto-shift.
    let non_text_region_file_offset = text_file_offset + text_placement.size;
    let non_text_result =
        compute_non_text_layouts(merged, member_keys, non_text_region_file_offset).map_err(
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
    // r506 — relative vtables follow the user strings; their
    // `total_size` carries the 8-align pad so the slots stay LDR-safe.
    let user_vtables_layout = build_user_vtables_region(
        &cfg.vtable_globals,
        TEXT_VMADDR_BASE,
        non_text_region_file_offset + non_text_region_size + user_strings_layout.total_size,
    );
    let text_size = text_placement.size
        + non_text_region_size
        + user_strings_layout.total_size
        + user_vtables_layout.total_size;

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
        text_placement,
        non_text_region_file_offset,
        non_text_per_member: non_text_result.per_member,
        non_text_region_size,
        user_strings_layout,
        user_strings_payload,
        user_vtables_layout,
        text_size,
        stubs_section_size,
        la_ptr_section_size,
        stubs_file_offset,
        text_vmsize,
    })
}

/// `__text`'s file offset, past the load commands, and where each
/// function lands in it, under [`text_shift`].
fn place_shifted_text(
    cfg: &LinkConfig,
    header_plus_lc: u32,
    member_text_sizes: &[u32],
) -> (u32, TextPlacement) {
    let (shift, user_gap) = text_shift();
    let offset = round_up_to(u64::from(header_plus_lc), u64::from(TEXT_FN_ALIGN)) as u32 + shift;
    let placement = place_text(
        cfg.funcs.iter().map(text_slot),
        member_text_sizes,
        user_gap,
        (offset, &cfg.user_loops),
    );
    (offset, placement.reported())
}

/// The widest gap [`text_shift`] takes: far past the 2 KiB period it
/// was made for, and small enough that no offset past it can wrap.
const MAX_TEXT_GAP: u32 = 1 << 20;

/// `TORAJS_LINK_TEXT_SHIFT=<bytes>[,<gap>]` (diagnostic): start
/// `__text` `<bytes>` (a multiple of 4 below 64) past its aligned slot,
/// and open the user functions `<gap>` bytes (a multiple of 64, at
/// most [`MAX_TEXT_GAP`]) past the last runtime member's fetch line.
/// The shift moves every function together, so each lands on a
/// different fetch-line phase. It cannot move user code against the
/// runtime, and a timing can hang on that distance alone, with a
/// period of 2 KiB: only the gap changes it. The loop pads are placed
/// after the shift and gap, and change with them; a scan of the bare
/// distance sets `TORAJS_LINK_LOOP_PLACE_OFF=1`. A value that does not
/// parse stops the link rather than measure something else.
fn text_shift() -> (u32, u32) {
    let Some(v) = std::env::var_os("TORAJS_LINK_TEXT_SHIFT") else {
        return (0, 0);
    };
    parse_text_shift(v.to_str().expect("TORAJS_LINK_TEXT_SHIFT is ASCII"))
}

fn parse_text_shift(v: &str) -> (u32, u32) {
    let (shift, gap) = v.split_once(',').unwrap_or((v, "0"));
    let shift: u32 = shift
        .parse()
        .expect("TORAJS_LINK_TEXT_SHIFT starts with a byte count");
    let gap: u32 = gap
        .parse()
        .expect("TORAJS_LINK_TEXT_SHIFT's gap is a byte count");
    assert!(
        shift.is_multiple_of(4) && shift < TEXT_FN_ALIGN,
        "TORAJS_LINK_TEXT_SHIFT must be a multiple of 4 below {TEXT_FN_ALIGN}"
    );
    assert!(
        gap.is_multiple_of(TEXT_FN_ALIGN) && gap <= MAX_TEXT_GAP,
        "TORAJS_LINK_TEXT_SHIFT's gap must be a multiple of {TEXT_FN_ALIGN} up to {MAX_TEXT_GAP}"
    );
    (shift, gap)
}

#[cfg(test)]
mod tests {
    use super::parse_text_shift;

    #[test]
    fn a_text_shift_reads_a_shift_and_an_optional_gap() {
        assert_eq!(parse_text_shift("8"), (8, 0));
        assert_eq!(parse_text_shift("8,0"), (8, 0));
        assert_eq!(parse_text_shift("8,128"), (8, 128));
    }

    #[test]
    fn a_text_shift_that_does_not_parse_stops_the_link() {
        for v in ["", "8,", "-1", "64", " 8", "8,100", "8,128,5", "0,2097152"] {
            assert!(
                std::panic::catch_unwind(|| parse_text_shift(v)).is_err(),
                "{v:?} parsed"
            );
        }
    }
}
