//! SD-4c swap-2e — `MemberLayout.defined_syms` final-vaddr dispatch.
//!
//! Extracted from `archive_emit.rs` to keep that file ≤ 500 prod
//! LOC (hard limit). Pure helper:
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

use crate::archive_emit::member_data_layouts;
use crate::archives_merge::MergedArchives;
use crate::class_name_table_layout::apply_class_name_table_overrides;
use crate::exec::LinkConfig;
use crate::fn_addr_syms::register_user_fn_syms;
use crate::fn_name_table_layout::apply_fn_name_table_overrides;
use crate::layout_types::ArchiveLayoutError;
use crate::tlv_descriptor_layout::apply_tlv_overrides;
use crate::user_class_layouts_layout::apply_user_class_layouts_overrides;
use crate::user_data_globals_layout::apply_user_data_global_overrides;
use crate::user_regex_baked_layout::apply_user_regex_baked_overrides;
use crate::user_strings_emit::apply_user_string_overrides;
use crate::user_vtables_layout::apply_user_vtable_overrides;
use crate::data_section_layout::DataSectionLayout;
use crate::layout_types::ArchiveLayout;
use crate::non_text_layout::NonTextSectionLayout;
use crate::resolve::{SymIndex, SymTable};

/// Dispatch each defined extern's final vaddr through its home
/// section. Mirrors [`crate::member_resolve::resolve_local_nsect`]
/// but operates on the simpler `defined_syms` path that doesn't
/// need the nlist re-read.
///
/// - `n_sect` matches a non-text section (`__cstring` / `__const`) →
///   `layout.final_vaddr + (n_value - layout.member_addr)`. swap-2k:
///   multi-section staticlibs (e.g. `libtorajs_fmt.a` packs `__text` +
///   `__const` + `__cstring` + `__DATA,__const`) place `__cstring`
///   at object-file `section.addr = 0x3814`; the LLVM-emit local sym
///   `l_anon.*` has `n_value = section.addr + intra_offset`. Without
///   the subtraction the ADRP+ADD target overshoots into the next
///   member's `__text` (read code bytes back as ASCII data — fmt_dtoa
///   returned `"2.3774616733207506"` instead of `"-Infinity"`).
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
        return layout.final_vaddr + n_value.saturating_sub(layout.member_addr);
    }
    if let Some(layout) = data_non_text_sections
        .iter()
        .find(|s| s.section_index == n_sect)
    {
        return layout.final_vaddr + n_value.saturating_sub(layout.member_addr);
    }
    member_vaddr + n_value
}

/// The `#[global_allocator]` shim family, matched by unmangled
/// suffix so the per-build `Cs<hash>_7___rustc` prefix does not
/// matter. `___rdl_alloc` deliberately does NOT match: that is
/// std's default System implementation, and the point of the
/// override is to stop binding to the shim that forwards to it.
fn is_allocator_shim_sym(name: &str) -> bool {
    const SHIMS: &[&str] = &[
        "___rust_alloc",
        "___rust_dealloc",
        "___rust_realloc",
        "___rust_alloc_zeroed",
        "___rust_alloc_error_handler",
        "___rust_no_alloc_shim_is_unstable_v2",
    ];
    SHIMS.iter().any(|s| name.ends_with(s))
}

/// Every member's defined externals into the vaddr table, each at the
/// address its home section gives it.
pub(crate) fn insert_member_syms(layout: &ArchiveLayout, effective_sym_table: &mut SymTable) {
    for (m_idx, m) in layout.member_layouts.iter().enumerate() {
        member_sym_vaddrs(layout, m_idx, &m.defined_syms, &mut |name, vaddr, first| {
            if first {
                effective_sym_table.entry(name.to_string()).or_insert(vaddr);
            } else {
                effective_sym_table.insert(name.to_string(), vaddr);
            }
        });
    }
}

/// [`insert_member_syms`] into a link's [`SymIndex`], `syms[i]` being
/// the defined externs of member `i` (taken out of the layout so the
/// index can borrow the names while the layout is still written to).
pub(crate) fn index_member_syms<'s>(
    layout: &ArchiveLayout,
    syms: &'s [Vec<(String, u64, u8)>],
    index: &mut SymIndex<'s>,
) {
    for (m_idx, defs) in syms.iter().enumerate() {
        member_sym_vaddrs(layout, m_idx, defs, &mut |name, vaddr, first| {
            if first {
                index.entry(name).or_insert(vaddr);
            } else {
                index.insert(name, vaddr);
            }
        });
    }
}

/// Each defined extern in `defs` of member `m_idx` of `layout` with its
/// final vaddr: `f(name, vaddr, first)`, `first` when an earlier
/// definition of the name keeps the table entry.
fn member_sym_vaddrs<'s>(
    layout: &ArchiveLayout,
    m_idx: usize,
    defs: &'s [(String, u64, u8)],
    f: &mut impl FnMut(&'s str, u64, bool),
) {
    let m = &layout.member_layouts[m_idx];
    let data_layouts = member_data_layouts(layout, m_idx);
    for (name, n_value, n_sect) in defs {
        let vaddr = section_vaddr_for_sym(
            *n_sect,
            *n_value,
            m.vaddr,
            &m.non_text_sections,
            data_layouts,
        );
        // First definition wins for the allocator shim family only.
        // Every `libtorajs_*.a` bundles its own `core`, so the default
        // `__rust_alloc -> __rdl_alloc` shim is defined once per
        // archive; plain `insert` lets the last archive scanned
        // decide, which is how user binaries ended up on libc malloc
        // while `TorajsAllocator` sat unreferenced in the image.
        // `member_layouts` is ordered by `(archive_idx, member_idx)`
        // (archive_link sorts `member_keys`), and
        // `libtorajs_panic_runtime.a` — the one crate carrying
        // `#[global_allocator]` — leads the runtime image, so
        // first-wins here means its shim is the one every call site
        // binds to. That matches what `merge_archive_indexes` already
        // documents for the symbol index (ld64 search order); the
        // vaddr table was the half that disagreed.
        f(name, vaddr, is_allocator_shim_sym(name));
    }
}

/// The names the link itself places — the dyld stubs, then the TLV
/// variables and the link's own tables — later ones winning.
pub(crate) fn link_defined_syms(
    layout: &ArchiveLayout,
    merged: &MergedArchives<'_>,
) -> Result<SymTable, ArchiveLayoutError> {
    let mut syms = SymTable::new();
    for (name, stub_vaddr) in &layout.stub_vaddrs {
        syms.insert(name.clone(), *stub_vaddr);
    }
    apply_tlv_overrides(layout, merged, &mut syms)?;
    apply_user_string_overrides(&layout.user_strings_layout, &mut syms);
    apply_user_data_global_overrides(&layout.user_data_globals_layout, &mut syms);
    apply_user_vtable_overrides(&layout.user_vtables_layout, &mut syms);
    apply_user_class_layouts_overrides(&layout.data_const_layout.class_layouts_layout, &mut syms);
    apply_fn_name_table_overrides(&layout.fn_name_table_layout, &mut syms);
    apply_class_name_table_overrides(&layout.class_name_table_layout, &mut syms);
    // C-5c.2d — outer `___torajs_baked_regex_<i>` syms point at each
    // entry's `BakedDfaMeta` block in __DATA_CONST. Inner `states_sym`
    // is not registered (reader goes through chain-LC rebased
    // `BakedDfaMeta::states_ptr` slot, see C-5c.2b/c).
    apply_user_regex_baked_overrides(&layout.data_const_layout.baked_regex_layout, &mut syms);
    Ok(syms)
}

/// Effective sym table = caller externs + member defined-externs
/// (vaddr dispatched by home section, `member_syms[i]` those of member
/// `i`) + user fns + `link_defined`, a later layer winning a name.
pub(crate) fn effective_syms<'s>(
    cfg: &'s LinkConfig,
    layout: &ArchiveLayout,
    member_syms: &'s [Vec<(String, u64, u8)>],
    link_defined: &'s SymTable,
) -> SymIndex<'s> {
    let n = cfg.sym_table.len()
        + member_syms.iter().map(Vec::len).sum::<usize>()
        + cfg.funcs.len()
        + link_defined.len();
    let mut index = SymIndex::with_capacity_and_hasher(n, Default::default());
    for (name, &vaddr) in &cfg.sym_table {
        index.insert(name, vaddr);
    }
    index_member_syms(layout, member_syms, &mut index);
    // S2-5 blade 2 prereq — user-fn names shadow member
    // defined-externs (user-first, matching the required-members walk
    // and dead-strip reachability, which both skip member pulls for
    // names in `defined_in_user`). Runs after the member sweep so a
    // same-named archive definition kept alive by an unrelated export
    // cannot win the vaddr table.
    register_user_fn_syms(&cfg.funcs, &layout.fn_vaddrs, &mut index);
    for (name, &vaddr) in link_defined {
        index.insert(name, vaddr);
    }
    index
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
            member_addr: 0,
            align: 0,
        }
    }

    fn text_layout_with_addr(
        section_index: u8,
        member_addr: u64,
        final_vaddr: u64,
    ) -> NonTextSectionLayout {
        NonTextSectionLayout {
            section_index,
            sectname: [0; 16],
            segname: [0; 16],
            member_internal_offset: 0,
            size: 0,
            final_file_offset: 0,
            final_vaddr,
            member_addr,
            align: 0,
        }
    }

    #[test]
    fn falls_back_to_text_when_no_section_matches() {
        let v = section_vaddr_for_sym(1, 0x42, 0x1_0000_4000, &[], &[]);
        assert_eq!(v, 0x1_0000_4042);
    }

    #[test]
    fn matches_non_text_section_zero_member_addr_no_op_subtraction() {
        // Legacy syscall/_malloc-style member: `__cstring` lives at
        // object-file addr 0 (single non-text section). n_value is
        // the intra-section offset; saturating_sub(0) is a no-op.
        let layout = text_layout(2, 0x1_0000_8000);
        let v = section_vaddr_for_sym(2, 0x42, 0xDEAD_BEEF, &[layout], &[]);
        assert_eq!(v, 0x1_0000_8042);
    }

    #[test]
    fn matches_non_text_section_subtracts_member_addr_fmt_case() {
        // swap-2k: libtorajs_fmt.a packs __text(0..0x1168) + __const
        // (0x1168..0x3814) + __cstring(0x3814..0x3891). An LLVM-emit
        // `l_anon.*` sym pointing at b"-Infinity" within __cstring has
        // n_value = 0x3814 + intra_offset. Without subtracting
        // member_addr the ADRP+ADD target overshoots into the next
        // member's __text, reading code bytes as ASCII.
        let layout = text_layout_with_addr(3, 0x3814, 0x1_0009_C000);
        let v = section_vaddr_for_sym(3, 0x3814 + 0x42, 0xDEAD_BEEF, &[layout], &[]);
        assert_eq!(v, 0x1_0009_C042);
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
