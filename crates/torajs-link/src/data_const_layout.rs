//! SD-4c-prereq+e8 — `__DATA_CONST` segment layout for the rodata
//! tables that hold ABSOLUTE pointers and so need dyld to stamp the
//! ASLR slide at load: class_layouts / fn_name_table /
//! class_name_table / baked regex. Apple's kernel codesigns `__TEXT`
//! pages — once dyld walks the chained-fixup chain and rewrites a
//! slot, the page hash diverges from what `LC_CODE_SIGNATURE`
//! recorded and the kernel terminates the process with `SIGKILL
//! (Code Signature Invalid)`. Every modern arm64 macOS binary
//! (swift / objc / rustc) moves such slots into a dedicated
//! `__DATA_CONST` segment instead: dyld can fix it up at load time,
//! and the `SG_READ_ONLY` segment flag tells the kernel to apply
//! `mprotect(PROT_READ)` after the fixup walk so the data is still
//! effectively immutable.
//!
//! `__vtable_<C>` slots lived here from e8 to r505. r506 made them
//! RELATIVE (`target - table_base`, see `user_vtables_layout`), which
//! needs no fixup — they are back in `__TEXT` rodata, and a program
//! whose only pointer table was its vtables no longer carries this
//! segment (one 16 KB page, the whole cost of a class override).
//!
//! Wire format references:
//! - `<mach-o/loader.h>` — `segment_command_64.flags` carries
//!   `SG_READ_ONLY = 0x10`
//! - Apple dyld source `dyld3/Loader.cpp` walks segments with
//!   `SG_READ_ONLY` and applies the post-fixup mprotect

use crate::class_name_table_layout::{
    ClassNameTableEntryLayout, ClassNameTableLayout, compute_class_name_table_layout,
};
use crate::fn_name_table_layout::{
    FnNameTableEntryLayout, FnNameTableLayout, compute_fn_name_table_layout,
};
use crate::lc::APPLE_SILICON_PAGE_SIZE;
use crate::user_class_layouts_layout::{UserClassLayoutsLayout, compute_user_class_layouts_layout};
use crate::user_regex_baked_layout::{UserRegexBakedLayout, compute_user_regex_baked_layout};
use torajs_obj::{SegmentCommand64, VM_PROT_READ, VM_PROT_WRITE};

/// `segment_command_64.flags` — segment becomes read-only after dyld
/// finishes applying chained fixups. The kernel codesign verifier
/// rehashes the page after dyld's mprotect, so the hash recorded in
/// `LC_CODE_SIGNATURE` matches the immutable post-fixup state and
/// runtime verification stays green.
pub const SG_READ_ONLY: u32 = 0x10;

/// Result of laying out the `__DATA_CONST` segment. When
/// `has_data_const` is false the caller skips emitting the
/// `LC_SEGMENT_64 __DATA_CONST` load command and downstream
/// segment indices stay unchanged from the pre-e8 layout.
#[derive(Debug, Clone, Default)]
pub struct DataConstLayout {
    /// SD-4c-prereq+e8 — per-class `child_offsets` tables for the
    /// cycle collector, at the segment base.
    pub class_layouts_layout: UserClassLayoutsLayout,
    /// Fn-name registry Phase 2 Step 3b.3 — `__torajs_fn_name_table[]`
    /// + `__torajs_fn_name_table_count` placed after class_layouts
    /// inside the same `__DATA_CONST` segment. Two chain-fixup slots
    /// per entry (`fn_addr`, `name_ptr`) need dyld rebase at load
    /// time, so the same `SG_READ_ONLY` post-mprotect path that hosts
    /// class_layouts hosts this region too. Empty layout (`total_size == 0`)
    /// when `cfg.fn_name_globals.is_empty()`.
    pub fn_name_table_layout: FnNameTableLayout,
    /// W-J Phase A3c — `__torajs_class_name_table[]` +
    /// `__torajs_n_class_names` placed after fn_name_table inside
    /// the same `__DATA_CONST` segment. One chain-fixup slot
    /// (`name_ptr`) per entry needs dyld rebase at load time, so
    /// the `SG_READ_ONLY` host pattern carries over. Empty layout
    /// (`total_size == 0`) when `cfg.class_names.is_empty()`.
    pub class_name_table_layout: ClassNameTableLayout,
    /// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-5c.2a — AOT-baked
    /// DFA region placed after class_name_table inside the same
    /// `__DATA_CONST` segment. Each `UserBakedRegexEntry` becomes a
    /// `(BakedDfaMeta, [DfaState; N])` pair; the `BakedDfaMeta::states_ptr`
    /// slot needs chain-LC rebase so the same `SG_READ_ONLY` post-mprotect
    /// path that hosts class_layouts hosts this region too.
    /// Empty layout (`total_size == 0`) when `cfg.baked_regex_entries`
    /// is empty — current state, since the SSA-side `Module` never
    /// pushes entries until Phase C-6 trips the AOT gate.
    pub baked_regex_layout: UserRegexBakedLayout,
    /// File offset of the `__DATA_CONST` segment's first byte.
    /// Equals `text_segment_file_offset + text_vmsize` in our layout.
    pub segment_file_offset: u32,
    /// Image-relative vmaddr of the segment's first byte. Equals
    /// `text_vmaddr + text_vmsize`.
    pub segment_vmaddr: u64,
    /// Page-aligned size of the segment's file payload. Equals
    /// `segment_vmsize` because every table here is file-backed (no
    /// zerofill).
    pub segment_filesize: u64,
    /// Page-aligned virtual size of the segment. dyld's chained-fixup
    /// `page_start[]` table walks every page in this range.
    pub segment_vmsize: u64,
    /// `false` when no table is present — the caller short-circuits
    /// every `__DATA_CONST` emit hook and the segment is not emitted.
    pub has_data_const: bool,
}

/// Does this config put anything into `__DATA_CONST`? The one
/// predicate both the load-command sizing (`archive_layout_text`) and
/// this layout consult — r499: the sizing used to test a shorter list
/// (no class-name table, no baked-regex region) and the mismatch was
/// invisible only while `tr build` forced every table on; the first
/// derived-off flag exposed it as a one-LC overrun on regex programs.
pub fn data_const_present(cfg: &crate::exec::LinkConfig) -> bool {
    !cfg.class_layouts.is_empty()
        || cfg.force_emit_class_layouts_globals
        || !cfg.fn_name_globals.is_empty()
        || cfg.force_emit_fn_name_globals
        || !cfg.class_names.is_empty()
        || cfg.force_emit_class_names_globals
        || !cfg.baked_regex_entries.is_empty()
}

/// Compute the `__DATA_CONST` segment layout. `segment_file_offset_base`
/// and `segment_vmaddr_base` are the file / virtual addresses of the
/// segment's first byte — `text_segment_file_offset + text_vmsize` and
/// `text_vmaddr + text_vmsize`. With every table empty the returned
/// layout has `has_data_const = false` and `segment_vmsize = 0`;
/// callers skip every `__DATA_CONST` emit path in that case.
pub fn compute_data_const_layout(
    class_layouts: &[crate::exec::UserClassLayoutEntry],
    force_emit_class_layouts_globals: bool,
    fn_name_globals: &[crate::exec::UserFnNameEntry],
    force_emit_fn_name_globals: bool,
    class_names: &[crate::exec::UserClassNameEntry],
    force_emit_class_names_globals: bool,
    baked_regex_entries: &[crate::exec::UserBakedRegexEntry],
    segment_file_offset_base: u32,
    segment_vmaddr_base: u64,
) -> DataConstLayout {
    if class_layouts.is_empty()
        && !force_emit_class_layouts_globals
        && fn_name_globals.is_empty()
        && !force_emit_fn_name_globals
        && class_names.is_empty()
        && !force_emit_class_names_globals
        && baked_regex_entries.is_empty()
    {
        return DataConstLayout {
            class_layouts_layout: UserClassLayoutsLayout::default(),
            fn_name_table_layout: FnNameTableLayout::default(),
            class_name_table_layout: ClassNameTableLayout::default(),
            baked_regex_layout: UserRegexBakedLayout::default(),
            segment_file_offset: segment_file_offset_base,
            segment_vmaddr: segment_vmaddr_base,
            segment_filesize: 0,
            segment_vmsize: 0,
            has_data_const: false,
        };
    }
    // class_layouts at the segment base, then fn_name_table, then
    // class_name_table, then baked regex. Each `compute_*_layout`
    // handles its own alignment + offset math from the running
    // cursor + vaddr (the segment base, so entries land at their
    // actual vaddrs).
    let class_layouts_cursor = segment_file_offset_base;
    let class_layouts_vaddr = segment_vmaddr_base;
    let class_layouts_layout = compute_user_class_layouts_layout(
        class_layouts,
        class_layouts_cursor,
        class_layouts_vaddr,
        force_emit_class_layouts_globals,
    );
    let fn_name_cursor = class_layouts_cursor + class_layouts_layout.total_size;
    let fn_name_vaddr = class_layouts_vaddr + u64::from(class_layouts_layout.total_size);
    let fn_name_entries: Vec<FnNameTableEntryLayout> = fn_name_globals
        .iter()
        .map(|e| FnNameTableEntryLayout {
            fn_addr_sym: e.fn_addr_sym.clone(),
            name_ptr_sym: e.name_ptr_sym.clone(),
            name_len: e.name_len,
            arity: e.arity,
            src_ptr_sym: e.src_ptr_sym.clone(),
            src_len: e.src_len,
        })
        .collect();
    let fn_name_table_layout = compute_fn_name_table_layout(
        fn_name_entries,
        fn_name_cursor,
        fn_name_vaddr,
        force_emit_fn_name_globals,
    );
    let class_name_cursor = fn_name_cursor + fn_name_table_layout.total_size;
    let class_name_vaddr = fn_name_vaddr + u64::from(fn_name_table_layout.total_size);
    let mut class_name_entries: Vec<ClassNameTableEntryLayout> = class_names
        .iter()
        .map(|e| ClassNameTableEntryLayout {
            class_tag: e.class_tag,
            name_ptr_sym: e.name_ptr_sym.clone(),
            name_len: e.name_len,
        })
        .collect();
    // Sorted by class_tag asc so the runtime binary-search invariant
    // holds. Empty Vec sort is a no-op.
    class_name_entries.sort_by_key(|e| e.class_tag);
    let class_name_table_layout = compute_class_name_table_layout(
        class_name_entries,
        class_name_cursor,
        class_name_vaddr,
        force_emit_class_names_globals,
    );
    let baked_regex_cursor = class_name_cursor + class_name_table_layout.total_size;
    let baked_regex_vaddr = class_name_vaddr + u64::from(class_name_table_layout.total_size);
    let baked_regex_layout =
        compute_user_regex_baked_layout(baked_regex_entries, baked_regex_cursor, baked_regex_vaddr);
    let total_region_size = class_layouts_layout.total_size
        + fn_name_table_layout.total_size
        + class_name_table_layout.total_size
        + baked_regex_layout.total_size;
    // Page-align so dyld's chained-fixup page walker can index from
    // the segment base — `page_start[]` always covers whole pages.
    let segment_vmsize =
        u64::from(total_region_size).div_ceil(APPLE_SILICON_PAGE_SIZE) * APPLE_SILICON_PAGE_SIZE;
    DataConstLayout {
        class_layouts_layout,
        fn_name_table_layout,
        class_name_table_layout,
        baked_regex_layout,
        segment_file_offset: segment_file_offset_base,
        segment_vmaddr: segment_vmaddr_base,
        segment_filesize: segment_vmsize,
        segment_vmsize,
        has_data_const: true,
    }
}

/// Build the `__DATA_CONST` `LC_SEGMENT_64` load command for the
/// emit pass. Returns `None` when `has_data_const` is false so the
/// caller can `let Some(s) = build_data_const_segment(layout) else { skip }`.
/// `maxprot/initprot = RW` lets dyld apply chained-fixup rebases;
/// `flags = SG_READ_ONLY` tells the kernel to `mprotect(PROT_READ)`
/// once dyld is done so codesigned page hashes stay valid.
pub fn build_data_const_segment(layout: &DataConstLayout) -> Option<SegmentCommand64> {
    if !layout.has_data_const {
        return None;
    }
    Some(SegmentCommand64 {
        segname: "__DATA_CONST".into(),
        vmaddr: layout.segment_vmaddr,
        vmsize: layout.segment_vmsize,
        fileoff: u64::from(layout.segment_file_offset),
        filesize: layout.segment_filesize,
        maxprot: VM_PROT_READ | VM_PROT_WRITE,
        initprot: VM_PROT_READ | VM_PROT_WRITE,
        flags: SG_READ_ONLY,
        sections: Vec::new(),
    })
}

/// Write the `__DATA_CONST` segment payload (class_layouts bytes if
/// any, then the name tables and baked regex). Pads the buf cursor
/// up to `segment_file_offset` first so the region lands where the
/// layout said. The `*_payload` slices are the byte streams the
/// `build_*_payload` helpers return — caller hands them in.
pub fn write_data_const_payload(
    buf: &mut Vec<u8>,
    layout: &DataConstLayout,
    user_class_layouts_payload: &[u8],
    fn_name_table_payload: &[u8],
    class_name_table_payload: &[u8],
    user_regex_baked_payload: &[u8],
) {
    if !layout.has_data_const {
        return;
    }
    let off = layout.segment_file_offset as usize;
    if buf.len() < off {
        buf.resize(off, 0);
    }
    buf.extend_from_slice(user_class_layouts_payload);
    buf.extend_from_slice(fn_name_table_payload);
    buf.extend_from_slice(class_name_table_payload);
    buf.extend_from_slice(user_regex_baked_payload);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::UserFnNameEntry;

    fn fn_name_entry(fid: u32, sid: u32, name_len: u32) -> UserFnNameEntry {
        UserFnNameEntry {
            fn_addr_sym: format!("__torajs_fn_{fid}"),
            name_ptr_sym: format!("__user_string_{sid}"),
            name_len,
            arity: 0,
            src_ptr_sym: None,
            src_len: 0,
        }
    }

    fn layout_with_fn_names(
        fn_names: &[UserFnNameEntry],
        file_offset_base: u32,
        vmaddr_base: u64,
    ) -> DataConstLayout {
        compute_data_const_layout(
            &[],
            false,
            fn_names,
            false,
            &[],
            false,
            &[],
            file_offset_base,
            vmaddr_base,
        )
    }

    #[test]
    fn empty_input_marks_segment_absent() {
        let layout = layout_with_fn_names(&[], 0x4000, 0x1_0000_4000);
        assert!(!layout.has_data_const);
        assert_eq!(layout.segment_vmsize, 0);
        assert_eq!(layout.segment_filesize, 0);
        assert!(layout.fn_name_table_layout.entries.is_empty());
    }

    #[test]
    fn fn_name_globals_alone_emits_one_page() {
        // Only fn_name_globals — the segment must still emit so the
        // runtime helper's externs resolve to a defined region, and
        // the table lands at the segment base.
        let fn_names = [fn_name_entry(0, 0, 3), fn_name_entry(1, 1, 5)];
        let layout = layout_with_fn_names(&fn_names, 0x4000, 0x1_0000_4000);
        assert!(layout.has_data_const);
        assert_eq!(layout.fn_name_table_layout.entries.len(), 2);
        // 2 × 40 entries + 8 count = 88; page-aligned to 0x4000.
        assert_eq!(layout.fn_name_table_layout.total_size, 88);
        assert_eq!(layout.segment_vmsize, 0x4000);
        assert_eq!(layout.segment_filesize, 0x4000);
        assert_eq!(layout.fn_name_table_layout.table_vaddr, 0x1_0000_4000);
    }

    #[test]
    fn segment_base_offsets_propagate_to_layout() {
        // file_offset_base / vmaddr_base mismatch — checks the helper
        // doesn't re-derive them from anywhere else.
        let fn_names = [fn_name_entry(0, 0, 3)];
        let layout = layout_with_fn_names(&fn_names, 0x8000, 0x1_0000_8000);
        assert_eq!(layout.segment_file_offset, 0x8000);
        assert_eq!(layout.segment_vmaddr, 0x1_0000_8000);
        assert_eq!(layout.fn_name_table_layout.table_vaddr, 0x1_0000_8000);
    }

    /// r499 — the sizing predicate and the layout's own emptiness
    /// test must agree on every table, or the load-command region
    /// overruns `text_file_offset` by one segment command. r506: a
    /// vtable-only config no longer puts anything here.
    #[test]
    fn presence_predicate_matches_layout_for_every_table() {
        let base = crate::exec::LinkConfig {
            funcs: Vec::new(),
            entry: "_main".into(),
            sym_table: crate::resolve::SymTable::new(),
            codesign_ident: "tora".into(),
            dead_strip: false,
            strip_member_symbols: false,
            elidable_sites: Vec::new(),
            guarded_stubs: Vec::new(),
            archives: Vec::new(),
            strings: Vec::new(),
            data_globals: Vec::new(),
            vtable_globals: Vec::new(),
            class_layouts: Vec::new(),
            force_emit_class_layouts_globals: false,
            fn_name_globals: Vec::new(),
            force_emit_fn_name_globals: false,
            class_names: Vec::new(),
            force_emit_class_names_globals: false,
            baked_regex_entries: Vec::new(),
        };
        let layout_of = |c: &crate::exec::LinkConfig| {
            compute_data_const_layout(
                &c.class_layouts,
                c.force_emit_class_layouts_globals,
                &c.fn_name_globals,
                c.force_emit_fn_name_globals,
                &c.class_names,
                c.force_emit_class_names_globals,
                &c.baked_regex_entries,
                0x4000,
                0x1_0000_4000,
            )
            .has_data_const
        };
        assert!(!data_const_present(&base));
        assert_eq!(layout_of(&base), data_const_present(&base));
        let mut only_vtables = base.clone();
        only_vtables
            .vtable_globals
            .push(crate::exec::UserVtableEntry {
                sym: "__vtable_A".into(),
                slot_syms: vec![Some("__torajs_fn_0".into())],
            });
        assert!(!data_const_present(&only_vtables));
        assert_eq!(layout_of(&only_vtables), data_const_present(&only_vtables));
        let mut only_class_names = base.clone();
        only_class_names.force_emit_class_names_globals = true;
        assert!(data_const_present(&only_class_names));
        assert_eq!(
            layout_of(&only_class_names),
            data_const_present(&only_class_names)
        );
        let mut only_fn_names = base.clone();
        only_fn_names.force_emit_fn_name_globals = true;
        assert_eq!(
            layout_of(&only_fn_names),
            data_const_present(&only_fn_names)
        );
    }

    #[test]
    fn sg_read_only_flag_matches_apple_loader_h() {
        // Locked-in constant — Apple's mach-o/loader.h defines
        // `SG_READ_ONLY = 0x10` and dyld branches on it to apply the
        // post-fixup mprotect.
        assert_eq!(SG_READ_ONLY, 0x10);
    }
}
