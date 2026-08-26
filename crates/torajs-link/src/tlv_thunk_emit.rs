//! TLV thunk slot patcher — SD-4c-prereq+c.
//!
//! Once `archive_emit::write_data_non_text_file_payloads`
//! has written each member's `__DATA,__thread_vars` section
//! verbatim, every TLV descriptor's `thunk` word is the raw
//! zero placeholder the source `.o` carries. Dyld won't bind
//! those slots through the chained-fixups blob unless the link
//! values [`crate::chained_fixups::build_chained_fixups`]
//! computed are present in the binary at the matching file
//! offsets.
//!
//! This module owns the byte-level patch pass. It walks the
//! pre-paired `(TlvDescriptorLayout, chain link value)` vecs
//! `ArchiveLayout` carries and overwrites each thunk slot
//! little-endian. Split out of `archive_emit` so that crate
//! stays under the 500-line module budget while the emit-side
//! TLV story has its own grep-able anchor.
//!
//! Address conversion model: the `__DATA` segment is contiguous
//! in file order, so
//! `file_offset = la_ptr_file_offset + (thunk_vaddr - data_vmaddr)`
//! maps each descriptor's `thunk_slot_vaddr` (an absolute image
//! address from `compute_data_section_layouts`) back to the
//! corresponding byte in the emit buffer. The buffer at this
//! point covers everything from the Mach-O header through the
//! end of the `__DATA` segment's file region, so the offset is
//! always in-bounds for non-empty `tlv_descriptors`.

use crate::layout_types::ArchiveLayout;

/// Overwrite each TLV descriptor's 8-byte `thunk` slot with the
/// pre-computed chained-fixups bind link. No-op when the
/// binary doesn't reference any thread-local variable
/// (`tlv_descriptors` is empty).
pub fn patch_tlv_thunk_slots(buf: &mut [u8], layout: &ArchiveLayout) {
    debug_assert_eq!(
        layout.tlv_descriptors.len(),
        layout.tlv_thunk_link_values.len(),
        "tlv_descriptors and tlv_thunk_link_values must be in lockstep",
    );
    for (d, link) in layout
        .tlv_descriptors
        .iter()
        .zip(layout.tlv_thunk_link_values.iter())
    {
        let file_offset = (u64::from(layout.la_ptr_file_offset)
            + (d.thunk_slot_vaddr - layout.data_vmaddr)) as usize;
        debug_assert!(
            file_offset + 8 <= buf.len(),
            "TLV thunk slot file offset {file_offset} + 8 exceeds buffer length {}",
            buf.len(),
        );
        buf[file_offset..file_offset + 8].copy_from_slice(&link.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tlv_descriptor_layout::TlvDescriptorLayout;
    use std::collections::BTreeMap;

    /// Build an `ArchiveLayout` with just the fields
    /// `patch_tlv_thunk_slots` touches; every other field gets
    /// a default the helper doesn't read.
    fn make_layout(
        la_ptr_file_offset: u32,
        data_vmaddr: u64,
        tlv_descriptors: Vec<TlvDescriptorLayout>,
        tlv_thunk_link_values: Vec<u64>,
    ) -> ArchiveLayout {
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
            la_ptr_file_offset,
            data_vmsize: 0,
            data_filesize: 0,
            data_vmaddr,
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
            data_merged_sections: Vec::new(),
            tlv_descriptors,
            tlv_thunk_link_values,
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

    fn descriptor(thunk_vaddr: u64) -> TlvDescriptorLayout {
        TlvDescriptorLayout {
            member_key: (0, 0),
            section_index: 1,
            descriptor_vaddr: thunk_vaddr,
            thunk_slot_vaddr: thunk_vaddr,
            key_slot_vaddr: thunk_vaddr + 8,
            offset_slot_vaddr: thunk_vaddr + 16,
        }
    }

    /// Empty TLV descriptor vec → buffer untouched.
    #[test]
    fn empty_tlv_layout_leaves_buf_unchanged() {
        let layout = make_layout(0x1000, 0x100000, Vec::new(), Vec::new());
        let original = vec![0xCDu8; 64];
        let mut buf = original.clone();
        patch_tlv_thunk_slots(&mut buf, &layout);
        assert_eq!(buf, original);
    }

    /// One thunk slot at vaddr-offset `0x10` from data_vmaddr,
    /// la_ptr_file_offset `0x100` → file_offset `0x110`. Helper
    /// overwrites exactly 8 bytes little-endian.
    #[test]
    fn single_thunk_writes_chain_link_le_at_translated_offset() {
        let layout = make_layout(
            0x100,
            0x100000,
            vec![descriptor(0x100010)],
            vec![0x8000_0000_0000_0042u64],
        );
        let mut buf = vec![0u8; 0x200];
        patch_tlv_thunk_slots(&mut buf, &layout);
        // file_offset = 0x100 + (0x100010 - 0x100000) = 0x110
        let written: [u8; 8] = buf[0x110..0x118].try_into().unwrap();
        assert_eq!(u64::from_le_bytes(written), 0x8000_0000_0000_0042);
        // bytes outside the 8-byte slot stay zero.
        assert!(buf[..0x110].iter().all(|&b| b == 0));
        assert!(buf[0x118..].iter().all(|&b| b == 0));
    }

    /// Multiple thunks → each lands at its own file offset and
    /// later writes don't clobber earlier ones (the slots are
    /// disjoint 8-byte regions in __DATA).
    #[test]
    fn multiple_thunks_write_each_to_its_own_slot() {
        let layout = make_layout(
            0x1000,
            0x100000,
            vec![
                descriptor(0x100020),
                descriptor(0x100100),
                descriptor(0x100200),
            ],
            vec![
                0x1111_2222_3333_4444,
                0x5555_6666_7777_8888,
                0x9999_AAAA_BBBB_CCCC,
            ],
        );
        let mut buf = vec![0u8; 0x2000];
        patch_tlv_thunk_slots(&mut buf, &layout);
        let read =
            |off: usize| -> u64 { u64::from_le_bytes(buf[off..off + 8].try_into().unwrap()) };
        // file_offset = 0x1000 + (thunk_vaddr - 0x100000)
        assert_eq!(read(0x1020), 0x1111_2222_3333_4444);
        assert_eq!(read(0x1100), 0x5555_6666_7777_8888);
        assert_eq!(read(0x1200), 0x9999_AAAA_BBBB_CCCC);
    }
}
