//! `__DATA` non-text section emit helpers — SD-4c-prereq-c-fix-c4.
//!
//! Pairs with [`crate::data_section_layout`] (which computes the
//! per-member layout) and [`crate::dyld_emit`] (which builds the
//! `__la_symbol_ptr` section + writes stubs). This module covers the
//! two emit-time artifacts fix-c4 adds:
//!
//! - Per-member `__DATA,*` `section_64` entries appended to the
//!   `__DATA` segment's section table (after `__la_symbol_ptr`).
//! - File payload bytes for the file-storage half of the member
//!   `__DATA,*` sections, written immediately after the
//!   `__la_symbol_ptr` slot table.
//!
//! Zerofill-family sections (`S_ZEROFILL` / `S_GB_ZEROFILL` /
//! `S_THREAD_LOCAL_ZEROFILL`) get a `section_64` entry too — with
//! `offset = 0` per Mach-O convention — but no file payload write.
//! Their bytes come into existence at load time when the loader
//! zero-inits the slice between the file region and the segment's
//! vmsize end.
//!
//! Lives in its own module to keep `archive_emit.rs` under the
//! 500-prod-LOC hard limit (the file is already at 430 prod LOC
//! after SD-2/3/4b).

use torajs_obj::Section64;

use crate::archive_link::ArchiveLayout;

/// Build one `Section64` entry per member `__DATA,*` section, in
/// the order [`crate::data_section_layout::compute_data_section_layouts`]
/// emits them (file-storage entries first across all members, then
/// zerofill entries across all members). Empty `Vec` when
/// `layout.data_non_text_layouts` is empty (no member contributes
/// `__DATA,*` sections — fast path stays SD-2-byte-identical).
///
/// `addr` is the final-binary vaddr (preserves the layout pass's
/// vaddr assignment). `size` mirrors `section_64.size` (vmsize for
/// zerofill, file size for regular). `offset` is the file offset
/// for file-storage sections, `0` for zerofill per Mach-O spec.
///
/// `flags` is copied verbatim from the source `section_64` so type
/// + attribute bits both round-trip. `align_log2` is copied from the
/// source `section_64.align` (swap-2i): the layout pass already
/// rounded `final_vaddr` to `1 << align`, so the emitted entry must
/// advertise the same alignment — e.g. the mmalloc `__common`/`__bss`
/// atomic-free-list globals declare `align 2^3` and would SIGBUS on an
/// `ldapr` if placed unaligned.
pub(crate) fn build_data_non_text_section_64_entries(layout: &ArchiveLayout) -> Vec<Section64> {
    let mut entries: Vec<Section64> = Vec::new();
    for per_member in &layout.data_non_text_layouts {
        for s in per_member {
            let offset = if s.has_file_storage {
                s.final_file_offset
            } else {
                0
            };
            entries.push(Section64 {
                sectname: trimmed_string(&s.sectname),
                segname: trimmed_string(&s.segname),
                addr: s.final_vaddr,
                size: u64::from(s.size),
                offset,
                align_log2: u32::from(s.align),
                reloff: 0,
                nreloc: 0,
                flags: s.flags,
                reserved1: 0,
                reserved2: 0,
                reserved3: 0,
            });
        }
    }
    entries
}

/// Write the file-storage payload bytes for every member's
/// `__DATA,*` regular section, in the same order
/// `build_data_non_text_section_64_entries` placed the file-
/// storage entries. Skips zerofill-family sections — the loader
/// supplies their bytes at startup.
///
/// `member_payloads` lines up 1:1 with the file-storage entries
/// (caller — `archive_emit::link_to_exec_with_archives` — collects
/// them by walking `layout.data_non_text_layouts` and slicing
/// `member.data[member_internal_offset..member_internal_offset +
/// size]` for each `has_file_storage == true` entry).
///
/// Pre-condition: `buf.len() == layout.la_ptr_file_offset +
/// layout.la_ptr_section_size` (= the first file-storage section's
/// region base). The la_ptr writer (`write_stubs_and_la_ptr`) leaves
/// the cursor there; this writer continues from that position,
/// zero-padding up to each section's `final_file_offset` before its
/// payload so the on-disk offset matches the alignment-rounded layout
/// (swap-2i — without the pad, an aligned `final_vaddr` would point
/// past the section's actual file bytes).
pub(crate) fn write_data_non_text_file_payloads(
    buf: &mut Vec<u8>,
    layout: &ArchiveLayout,
    member_payloads: &[Vec<u8>],
) {
    let mut pi = 0;
    for per_member in &layout.data_non_text_layouts {
        for s in per_member {
            if !s.has_file_storage {
                continue;
            }
            // pad to the alignment-rounded file offset; pads are tiny
            // (< 1 << align) so a push loop is cheap.
            while (buf.len() as u32) < s.final_file_offset {
                buf.push(0);
            }
            buf.extend_from_slice(&member_payloads[pi]);
            pi += 1;
        }
    }
    debug_assert_eq!(
        pi,
        member_payloads.len(),
        "payload count must match file-storage section count",
    );
}

fn trimmed_string(slot: &[u8; 16]) -> String {
    let mut end = slot.len();
    while end > 0 && (slot[end - 1] == 0 || slot[end - 1] == b' ') {
        end -= 1;
    }
    String::from_utf8_lossy(&slot[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_section_layout::DataSectionLayout;
    use crate::layout_types::ArchiveLayout;
    use std::collections::BTreeMap;

    fn empty_archive_layout() -> ArchiveLayout {
        ArchiveLayout {
            text_file_offset: 0,
            text_size: 0,
            text_vmaddr: 0,
            text_vmsize: 0,
            linkedit_file_offset: 0,
            linkedit_vmaddr: 0,
            linkedit_vmsize: 0,
            symoff: 0,
            stroff: 0,
            total_size: 0,
            nsyms: 0,
            strsize: 0,
            entry_file_offset: 0,
            fn_vaddrs: Vec::new(),
            member_layouts: Vec::new(),
            codesign_dataoff: 0,
            codesign_datasize: 0,
            dyld_imports: BTreeMap::new(),
            has_dyld: false,
            stubs_section_vaddr: 0,
            stubs_section_size: 0,
            stubs_file_offset: 0,
            la_ptr_section_vaddr: 0,
            la_ptr_section_size: 0,
            la_ptr_file_offset: 0,
            data_vmsize: 0,
            data_filesize: 0,
            data_vmaddr: 0,
            stub_vaddrs: BTreeMap::new(),
            chained_fixups_blob: Vec::new(),
            chained_fixups_dataoff: 0,
            chained_fixups_datasize: 0,
            la_ptr_slot_values: Vec::new(),
            non_text_region_file_offset: 0,
            non_text_region_size: 0,
            data_non_text_layouts: Vec::new(),
            data_non_text_file_offset: 0,
            data_non_text_file_size: 0,
            data_non_text_zerofill_vmsize: 0,
            tlv_descriptors: Vec::new(),
            tlv_thunk_link_values: Vec::new(),
            user_strings_layout: Default::default(),
            user_strings_payload: Vec::new(),
            user_data_globals_layout: Default::default(),
            user_vtables_layout: Default::default(),
            fn_name_table_layout: Default::default(),
            class_name_table_layout: Default::default(),
            data_const_layout: Default::default(),
            text_rebase_link_values: Vec::new(),
            vtable_rebase_target_count: 0,
            fn_name_rebase_target_count: 0,
            class_name_rebase_target_count: 0,
            baked_regex_rebase_target_count: 0,
        }
    }

    fn padded_name(s: &[u8]) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[..s.len()].copy_from_slice(s);
        out
    }

    #[test]
    fn empty_layout_produces_no_entries() {
        let layout = empty_archive_layout();
        let entries = build_data_non_text_section_64_entries(&layout);
        assert!(entries.is_empty());
    }

    #[test]
    fn entries_preserve_flags_and_pick_offset_per_storage() {
        let mut layout = empty_archive_layout();
        layout.data_non_text_layouts = vec![vec![
            DataSectionLayout {
                section_index: 3,
                sectname: padded_name(b"__data"),
                segname: padded_name(b"__DATA"),
                member_internal_offset: 100,
                size: 16,
                flags: torajs_obj::S_REGULAR,
                has_file_storage: true,
                final_file_offset: 0x6000,
                final_vaddr: 0xDA00_0000,
                member_addr: 0,
                align: 3,
            },
            DataSectionLayout {
                section_index: 4,
                sectname: padded_name(b"__common"),
                segname: padded_name(b"__DATA"),
                member_internal_offset: 0,
                size: 2048,
                flags: torajs_obj::S_ZEROFILL,
                has_file_storage: false,
                final_file_offset: 0,
                final_vaddr: 0xDA00_0010,
                member_addr: 0,
                align: 3,
            },
        ]];
        let entries = build_data_non_text_section_64_entries(&layout);
        assert_eq!(entries.len(), 2);
        // File-storage entry: offset = final_file_offset, size = 16.
        assert_eq!(entries[0].sectname, "__data");
        assert_eq!(entries[0].segname, "__DATA");
        assert_eq!(entries[0].addr, 0xDA00_0000);
        assert_eq!(entries[0].size, 16);
        assert_eq!(entries[0].offset, 0x6000);
        assert_eq!(entries[0].flags, torajs_obj::S_REGULAR);
        // Zerofill entry: offset MUST be 0 per Mach-O spec.
        assert_eq!(entries[1].sectname, "__common");
        assert_eq!(entries[1].segname, "__DATA");
        assert_eq!(entries[1].addr, 0xDA00_0010);
        assert_eq!(entries[1].size, 2048);
        assert_eq!(entries[1].offset, 0, "zerofill offset MUST be 0");
        assert_eq!(entries[1].flags, torajs_obj::S_ZEROFILL);
    }

    fn file_storage_layout(
        sectname: &[u8],
        size: u32,
        final_file_offset: u32,
        align: u8,
    ) -> DataSectionLayout {
        DataSectionLayout {
            section_index: 1,
            sectname: padded_name(sectname),
            segname: padded_name(b"__DATA"),
            member_internal_offset: 0,
            size,
            flags: torajs_obj::S_REGULAR,
            has_file_storage: true,
            final_file_offset,
            final_vaddr: 0,
            member_addr: 0,
            align,
        }
    }

    /// Writer zero-pads up to each section's alignment-rounded
    /// `final_file_offset` before its payload (swap-2i): section B
    /// declares 16-byte align so the writer inserts an 8-byte pad
    /// after A (which ends at 24) to land B at 32; C declares 8-byte
    /// align so a 4-byte pad lands it at 40.
    #[test]
    fn write_payloads_pads_to_aligned_file_offsets() {
        let mut layout = empty_archive_layout();
        layout.data_non_text_layouts = vec![vec![
            file_storage_layout(b"__data", 8, 16, 0),
            file_storage_layout(b"__const", 4, 32, 4),
            file_storage_layout(b"__bar", 4, 40, 3),
        ]];
        let mut buf: Vec<u8> = vec![0xAA; 16]; // simulate la_ptr already written
        let payloads = vec![vec![0x11; 8], vec![0x22; 4], vec![0x33; 4]];
        write_data_non_text_file_payloads(&mut buf, &layout, &payloads);
        assert_eq!(buf.len(), 44);
        assert_eq!(&buf[16..24], &[0x11; 8], "section A payload");
        assert_eq!(&buf[24..32], &[0u8; 8], "pad before 16-aligned B");
        assert_eq!(&buf[32..36], &[0x22; 4], "section B payload");
        assert_eq!(&buf[36..40], &[0u8; 4], "pad before 8-aligned C");
        assert_eq!(&buf[40..44], &[0x33; 4], "section C payload");
    }
}
