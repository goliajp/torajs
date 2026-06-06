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
///    zerofill).
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
        Some(layout.final_vaddr + n_value)
    } else if n_sect == 1 {
        Some(member_vaddr + n_value)
    } else {
        None
    }
}
