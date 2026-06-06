//! SD-4c swap-2e — `MemberLayout.defined_syms` final-vaddr dispatch.
//!
//! Extracted from `archive_emit.rs` to keep that file ≤ 500 prod
//! LOC (HARD per `.claude/rules/common/file-size.md`). Pure helper:
//! takes one `(n_sect, n_value)` slot from a member's nlist plus the
//! pre-computed section placements and returns the symbol's final
//! virtual address.
//!
//! ## Why a separate dispatch
//!
//! Pre-2e `archive_emit` registered every member-defined extern at
//! `member.vaddr + n_value`, blindly assuming the symbol lives in
//! the member's `__TEXT,__text` section. rustc-emit statics
//! (`static G_BUFFER_LEN: AtomicU32 = AtomicU32::new(0);` etc.) live
//! in `__DATA,__bss` / `__common`; their nlist `n_value` is the
//! section-relative offset (rustc emits each non-text section at
//! the next alignment slot, so `section.addr` is non-zero — cycle's
//! `__common` lands at `0xa28`), not a `__text` offset. The naive
//! formula landed G_BUFFER_LEN inside another function's prologue
//! bytes (4 bytes after __torajs_obj_drop_sized's tail-call B), the
//! cbz w8 early-out in __torajs_cycle_collect read non-zero,
//! iteration walked garbage, and the leaf fixture SIGSEGV'd at
//! `cycle_collect + 84`.

use crate::data_section_layout::DataSectionLayout;
use crate::non_text_layout::NonTextSectionLayout;

/// Dispatch each defined extern's final vaddr through its home
/// section. Mirrors [`crate::member_resolve::resolve_local_nsect`]
/// but operates on the simpler `defined_syms` path that doesn't
/// need the nlist re-read.
///
/// - `n_sect` matches a non-text section (`__cstring` / `__const`) →
///   `layout.final_vaddr + n_value` (matches legacy
///   `resolve_local_nsect`; `NonTextSectionLayout` doesn't carry
///   `section.addr` and `__TEXT,*` non-text sections didn't surface
///   the rustc non-zero-addr case in the current probe suite).
/// - `n_sect` matches a `__DATA,*` section → `layout.final_vaddr +
///   (n_value - layout.member_addr)` (recovers the intra-section
///   offset rustc encodes as `section.addr + offset`).
/// - No match → fall back to `member_vaddr + n_value`
///   (`__TEXT,__text` convention; rustc / clang emit `__text` as
///   the first section so `compute_non_text_layouts` filters it out).
pub(crate) fn section_vaddr_for_sym(
    n_sect: u8,
    n_value: u64,
    member_vaddr: u64,
    non_text_sections: &[NonTextSectionLayout],
    data_non_text_sections: &[DataSectionLayout],
) -> u64 {
    if let Some(layout) = non_text_sections.iter().find(|s| s.section_index == n_sect) {
        return layout.final_vaddr + n_value;
    }
    if let Some(layout) = data_non_text_sections
        .iter()
        .find(|s| s.section_index == n_sect)
    {
        return layout.final_vaddr + n_value.saturating_sub(layout.member_addr);
    }
    member_vaddr + n_value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_layout(section_index: u8, member_addr: u64, final_vaddr: u64) -> DataSectionLayout {
        DataSectionLayout {
            section_index,
            sectname: [0; 16],
            segname: [0; 16],
            member_internal_offset: 0,
            size: 0,
            flags: 0,
            has_file_storage: true,
            final_file_offset: 0,
            final_vaddr,
            member_addr,
            align: 0,
        }
    }

    fn text_layout(section_index: u8, final_vaddr: u64) -> NonTextSectionLayout {
        NonTextSectionLayout {
            section_index,
            sectname: [0; 16],
            segname: [0; 16],
            member_internal_offset: 0,
            size: 0,
            final_file_offset: 0,
            final_vaddr,
        }
    }

    #[test]
    fn falls_back_to_text_when_no_section_matches() {
        let v = section_vaddr_for_sym(1, 0x42, 0x1_0000_4000, &[], &[]);
        assert_eq!(v, 0x1_0000_4042);
    }

    #[test]
    fn matches_non_text_section_no_member_addr_subtraction() {
        let layout = text_layout(2, 0x1_0000_8000);
        let v = section_vaddr_for_sym(2, 0x42, 0xDEAD_BEEF, &[layout], &[]);
        assert_eq!(v, 0x1_0000_8042);
    }

    #[test]
    fn matches_data_section_subtracts_member_addr() {
        // rustc emits __common at member_addr = 0xa28 (after __text).
        // n_value = 0xa28 means offset 0 within __common.
        let layout = data_layout(3, 0xa28, 0x1_0030_5000);
        let v = section_vaddr_for_sym(3, 0xa28, 0xDEAD_BEEF, &[], &[layout]);
        assert_eq!(v, 0x1_0030_5000);
    }

    #[test]
    fn data_section_offset_beyond_member_addr_added() {
        // Symbol at member_addr + 0x10 within the data section.
        let layout = data_layout(3, 0x1000, 0x2_0000_0000);
        let v = section_vaddr_for_sym(3, 0x1010, 0xDEAD_BEEF, &[], &[layout]);
        assert_eq!(v, 0x2_0000_0010);
    }

    #[test]
    fn saturating_sub_prevents_underflow_on_corrupt_input() {
        // Defensive: if n_value < member_addr (malformed member),
        // clamp the offset to 0 rather than wrapping to a huge value.
        let layout = data_layout(3, 0x1000, 0x2_0000_0000);
        let v = section_vaddr_for_sym(3, 0x500, 0xDEAD_BEEF, &[], &[layout]);
        assert_eq!(v, 0x2_0000_0000);
    }
}
