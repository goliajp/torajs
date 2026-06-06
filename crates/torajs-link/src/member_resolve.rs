//! Member-internal local-symbol resolver — extracted from
//! `member_apply` per file-size hard rule
//! (SD-4c-prereq-c-fix-c5-extract).
//!
//! Hosts [`resolve_local_nsect`] (used by
//! `member_apply::apply_member_relocs` for `r_extern=1` relocs whose
//! target is a `N_SECT` local symbol — rustc's `l_anon.*` panic-
//! string labels, etc.). Pure function — no state, no behaviour
//! change vs the pre-extract inline version.

use torajs_obj::NLIST_64_SIZE;

use crate::archive::ArMember;
use crate::data_section_layout::DataSectionLayout;
use crate::non_text_layout::NonTextSectionLayout;

/// Sub-view of a parsed `LC_SYMTAB`, the four absolute file offsets
/// + counts the symbol-name resolver needs. `pub(crate)` so
/// `member_apply::parse_member_symtab` (still owns the parse) and
/// `resolve_local_nsect` (this module) share the struct.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MemberSymtab {
    pub(crate) symoff: u32,
    pub(crate) nsyms: u32,
    pub(crate) stroff: u32,
    pub(crate) strsize: u32,
}

/// Resolve a member's reloc that targets a *local* `N_SECT` symbol
/// (a name in the member's symtab whose `n_type` is `N_SECT` with no
/// `N_EXT` bit — e.g. rustc's `l_anon.*` panic-string labels). The
/// link driver's `sym_table` only carries externally-exported names,
/// so locals fall through to this lookup.
///
/// Returns `None` when the nlist entry isn't a local section symbol
/// or the target section isn't in any of the supplied layouts.
///
/// Lookup order:
/// 1. `non_text_sections` (fix-c1 — only `__TEXT,*` regular).
/// 2. `data_non_text_sections` (fix-c2/c5 — `__DATA,*` regular +
///    zerofill). The symbol's `n_value` is its *object-file* address
///    (`section.addr + intra_offset`); rustc places `__DATA,*`
///    sections after `__text` so `section.addr` (= `member_addr`) is
///    non-zero. Subtract it to recover the intra-section offset before
///    adding the final placement — mirrors swap-2e's
///    [`crate::defined_extern_resolve::section_vaddr_for_sym`]. Without
///    the subtraction a `__bss`-merged static (e.g. LLVM's
///    `__MergedGlobals` holding `UNHANDLED_REJECTION_OCCURRED`)
///    resolves to `final_vaddr + section.addr`, overshooting the
///    `__DATA` segment into `__LINKEDIT` and reading garbage.
/// 3. `n_sect == 1` → assume `__TEXT,__text` and use `member_vaddr`
///    (rustc convention; `compute_non_text_layouts` filters __text
///    out so the first lookup misses).
pub(crate) fn resolve_local_nsect(
    member: &ArMember<'_>,
    symtab: &MemberSymtab,
    r_symbolnum: u32,
    member_vaddr: u64,
    non_text_sections: &[NonTextSectionLayout],
    data_non_text_sections: &[DataSectionLayout],
) -> Option<u64> {
    const N_TYPE_MASK: u8 = 0x0E;
    const N_SECT_TYPE: u8 = 0x0E;
    let bytes = member.data;
    let nlist_off = symtab.symoff as usize + r_symbolnum as usize * NLIST_64_SIZE as usize;
    let n_type = bytes[nlist_off + 4];
    let n_sect = bytes[nlist_off + 5];
    if (n_type & N_TYPE_MASK) != N_SECT_TYPE || n_sect == 0 {
        return None;
    }
    let n_value = u64::from_le_bytes(bytes[nlist_off + 8..nlist_off + 16].try_into().unwrap());
    if let Some(layout) = non_text_sections.iter().find(|s| s.section_index == n_sect) {
        Some(layout.final_vaddr + n_value)
    } else if let Some(layout) = data_non_text_sections
        .iter()
        .find(|s| s.section_index == n_sect)
    {
        Some(layout.final_vaddr + n_value.saturating_sub(layout.member_addr))
    } else if n_sect == 1 {
        Some(member_vaddr + n_value)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a one-entry symtab buffer with an `N_SECT` local nlist
    /// (`n_sect`, `n_value`) at symbol index 0. nlist_64 layout:
    /// n_strx(4) n_type(1) n_sect(1) n_desc(2) n_value(8).
    fn member_with_nsect(n_type: u8, n_sect: u8, n_value: u64) -> Vec<u8> {
        let mut data = vec![0u8; NLIST_64_SIZE as usize];
        data[4] = n_type;
        data[5] = n_sect;
        data[8..16].copy_from_slice(&n_value.to_le_bytes());
        data
    }

    fn symtab() -> MemberSymtab {
        MemberSymtab {
            symoff: 0,
            nsyms: 1,
            stroff: 0,
            strsize: 0,
        }
    }

    fn data_layout(section_index: u8, member_addr: u64, final_vaddr: u64) -> DataSectionLayout {
        DataSectionLayout {
            section_index,
            sectname: [0; 16],
            segname: [0; 16],
            member_internal_offset: 0,
            size: 0,
            flags: 0,
            has_file_storage: false,
            final_file_offset: 0,
            final_vaddr,
            member_addr,
            align: 0,
        }
    }

    /// A `__bss`-merged static (LLVM `__MergedGlobals`) at section 6,
    /// `n_value = section.addr = 0x23c8`, must resolve to the section's
    /// final vaddr with offset 0 — not `final_vaddr + 0x23c8`, which
    /// overshoots into `__LINKEDIT`. Regression for the swap-2f leaf
    /// fixture exit-1 (`UNHANDLED_REJECTION_OCCURRED` read garbage).
    #[test]
    fn data_section_subtracts_member_addr() {
        const N_SECT_TYPE: u8 = 0x0E;
        let data = member_with_nsect(N_SECT_TYPE, 6, 0x23c8);
        let member = ArMember {
            name: "cgu.o",
            data: &data,
        };
        let layout = data_layout(6, 0x23c8, 0x1_0031_6264);
        let v = resolve_local_nsect(&member, &symtab(), 0, 0xDEAD_BEEF, &[], &[layout]);
        assert_eq!(v, Some(0x1_0031_6264));
    }

    /// Intra-section offset is preserved: a symbol at
    /// `member_addr + 0x10` lands at `final_vaddr + 0x10`.
    #[test]
    fn data_section_preserves_intra_offset() {
        const N_SECT_TYPE: u8 = 0x0E;
        let data = member_with_nsect(N_SECT_TYPE, 4, 0x2408);
        let member = ArMember {
            name: "cgu.o",
            data: &data,
        };
        let layout = data_layout(4, 0x2398, 0x2_0000_0000);
        let v = resolve_local_nsect(&member, &symtab(), 0, 0xDEAD_BEEF, &[], &[layout]);
        assert_eq!(v, Some(0x2_0000_0070));
    }

    /// Non-`N_SECT` entries (e.g. undefined externs) return `None` so
    /// the caller falls through to the global sym-table lookup.
    #[test]
    fn non_nsect_returns_none() {
        let data = member_with_nsect(0x00, 6, 0x23c8); // N_UNDF
        let member = ArMember {
            name: "cgu.o",
            data: &data,
        };
        let layout = data_layout(6, 0x23c8, 0x1_0031_6264);
        let v = resolve_local_nsect(&member, &symtab(), 0, 0xDEAD_BEEF, &[], &[layout]);
        assert_eq!(v, None);
    }
}
