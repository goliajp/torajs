//! Layout-pass type definitions extracted from
//! [`crate::archive_link`] so that file can stay under the
//! 500-prod-LOC hard limit after SD-4c-prereq-b2 added the
//! `non_text_sections` / `non_text_region_*` fields.
//!
//! Pure data structures — no behaviour, no helpers. The
//! `compute_archive_layout` function (which populates these) stays
//! in `archive_link.rs`; consumers (`archive_emit.rs`, link drivers)
//! import the types through the public lib re-exports.

use std::collections::BTreeMap;

use crate::archive::MemberSymtabError;
use crate::archives_merge::{ArchiveLinkError, ArchiveMergeError};
use crate::data_section_layout::DataSectionLayout;
use crate::fn_name_table_layout::FnNameTableLayout;
use crate::member_apply::MemberRelocApplyError;
use crate::member_reloc::MemberRelocError;
use crate::member_text::MemberTextError;
use crate::non_text_layout::NonTextSectionLayout;
use crate::tlv_descriptor_layout::{TlvDescriptorLayout, TlvSymOverrideError};
use crate::user_data_globals_layout::UserDataGlobalsLayout;
use crate::user_strings_layout::UserStringsLayout;
use crate::user_vtables_layout::UserVtablesLayout;

/// Per-archive-member computed data the S7-C4 emit pass needs.
/// `defined_syms` carries `(name, n_value, n_sect)` exactly as the
/// member's `LC_SYMTAB` records them — `n_value` is section-relative
/// inside the symbol's home section (which `n_sect` identifies).
/// archive_emit dispatches the final-binary vaddr by looking the
/// section up: `n_sect = 1` (__text) → `member.vaddr + n_value`;
/// other sections → `non_text_sections` / `data_non_text_layouts`
/// final_vaddr + (n_value - section.member_addr).
#[derive(Debug, Clone)]
pub struct MemberLayout {
    /// `(archive_idx, member_idx)` into `cfg.archives` /
    /// `MergedArchives::per_archive_members`.
    pub key: (usize, usize),
    /// Byte length of the member's `__TEXT,__text` payload.
    pub text_size: u32,
    /// Final virtual address where the member's `__text` lands.
    pub vaddr: u64,
    /// File offset within `__TEXT,__text` of the member's payload.
    pub file_offset: u32,
    /// File offset of the member's `__text` *inside the `.o`*.
    pub member_text_offset_in_member: u32,
    /// Defined-external symbols the member exports.
    pub defined_syms: Vec<(String, u64, u8)>,
    /// SD-4c-prereq-b2 — non-`__text` sections (cstring/const/data)
    /// with shadow final-binary placements for the SD-4c-prereq-b3
    /// SectionRef arm. See [`NonTextSectionLayout`] for the
    /// shadow-vs-real-layout semantics.
    pub non_text_sections: Vec<NonTextSectionLayout>,
}

/// Layout decisions for an archive-aware link — superset of
/// `ExecLayout` with per-member integration data.
#[derive(Debug, Clone)]
pub struct ArchiveLayout {
    pub text_file_offset: u32,
    pub text_size: u32,
    pub text_vmaddr: u64,
    pub text_vmsize: u64,
    pub linkedit_file_offset: u32,
    pub linkedit_vmaddr: u64,
    pub linkedit_vmsize: u64,
    pub symoff: u32,
    pub stroff: u32,
    pub total_size: u32,
    pub nsyms: u32,
    pub strsize: u32,
    pub entry_file_offset: u32,
    pub fn_vaddrs: Vec<u64>,
    /// Per integrated member, in sorted `(archive_idx, member_idx)` order.
    pub member_layouts: Vec<MemberLayout>,
    pub codesign_dataoff: u32,
    pub codesign_datasize: u32,
    /// SD-1 / SD-4b — `name → lib_ordinal` map classified by
    /// `dyld_syms::dyld_lib_for` during the worklist closure.
    pub dyld_imports: BTreeMap<String, u8>,
    /// The dyld/`__DATA` layout exists. True whenever imports are
    /// non-empty OR the program carries user data globals: a macOS
    /// userland executable links libSystem by platform contract
    /// (ld64 enforces the same), so a fully-specialized program
    /// whose member closure needs zero libc keeps the LC_LOAD_DYLIB
    /// + `__DATA` scaffolding with an empty bind set rather than
    /// degrading to the probe-only no-dyld shape. Emit-side
    /// consumers read THIS field instead of re-deriving from
    /// `dyld_imports` (the re-derivations disagreed once the
    /// zero-import shape became reachable).
    pub has_dyld: bool,
    pub stubs_section_vaddr: u64,
    pub stubs_section_size: u64,
    pub stubs_file_offset: u32,
    pub la_ptr_section_vaddr: u64,
    pub la_ptr_section_size: u64,
    pub la_ptr_file_offset: u32,
    /// vmsize of the `__DATA` segment — covers `__la_symbol_ptr`
    /// plus any member `__DATA,*` file-storage + zerofill payloads.
    /// Page-aligned per Mach-O segment-vmsize convention.
    pub data_vmsize: u64,
    /// file size of the `__DATA` segment — covers `__la_symbol_ptr`
    /// plus member `__DATA,*` *file-storage* payloads only (zerofill
    /// sections occupy vmsize beyond filesize). Page-aligned so the
    /// next segment's fileoff stays page-aligned too. SD-4c-prereq-
    /// c-fix-c4 — before fix-c4 the segment had no zerofill so
    /// filesize == vmsize trivially.
    pub data_filesize: u64,
    pub data_vmaddr: u64,
    /// SD-2a — per-import stub vaddr (sorted by name).
    pub stub_vaddrs: BTreeMap<String, u64>,
    /// SD-3 — assembled `LC_DYLD_CHAINED_FIXUPS` blob inside `__LINKEDIT`.
    pub chained_fixups_blob: Vec<u8>,
    pub chained_fixups_dataoff: u32,
    pub chained_fixups_datasize: u32,
    /// SD-3 — per-import 8-byte chain-link value for `__la_symbol_ptr`.
    pub la_ptr_slot_values: Vec<u64>,
    /// SD-4c-prereq-b2 shadow numbers — start file offset + total
    /// size of the future non-text region. SD-4c-prereq-c uses
    /// these to shift `stubs_file_offset` and emit the payloads.
    pub non_text_region_file_offset: u32,
    pub non_text_region_size: u32,
    /// SD-4c-prereq-c-fix-c3 — per-member `__DATA,*` section
    /// placements (file-storage entries first, then zerofill).
    /// Empty outer Vec when `has_dyld == false` (no `__DATA`
    /// segment exists). fix-c4 walks this to emit section_64
    /// entries + file payload.
    pub data_non_text_layouts: Vec<Vec<DataSectionLayout>>,
    /// SD-4c-prereq-c-fix-c3 shadow number — start file offset of
    /// the `__DATA` non-text file region (immediately after
    /// `__la_symbol_ptr`). fix-c4 wires this into the emit pass +
    /// extends `linkedit_file_offset` past it.
    pub data_non_text_file_offset: u32,
    /// SD-4c-prereq-c-fix-c3 shadow number — total bytes the
    /// file-storage half of `__DATA` non-text occupies. fix-c4
    /// extends `data_vmsize` / `linkedit_file_offset` by this.
    pub data_non_text_file_size: u32,
    /// SD-4c-prereq-c-fix-c3 shadow number — additional vmsize the
    /// zerofill half needs past the file region (loader supplies
    /// zero-init at startup; no file payload). fix-c4 extends
    /// `data_vmsize` by this.
    pub data_non_text_zerofill_vmsize: u32,
    /// SD-4c-prereq+b1 — enumerated TLV descriptors across every
    /// integrated member's `__DATA,__thread_vars` sections. Empty
    /// when no member contributes a TLV descriptor table (syscall-
    /// abort / `_malloc` probes). SD-4c-prereq+c walks this to add
    /// per-thunk-slot bind entries to the chained-fixups blob.
    pub tlv_descriptors: Vec<TlvDescriptorLayout>,
    /// SD-4c-prereq+c — per-TLV-descriptor 8-byte chain-link
    /// value the emit pass writes onto each `thunk` slot in the
    /// binary. Indexed in the same order as `tlv_descriptors`;
    /// the emit pass walks both vecs in lockstep so it doesn't
    /// have to re-derive the offset → link encoding. Empty when
    /// `tlv_descriptors` is empty.
    pub tlv_thunk_link_values: Vec<u64>,
    /// SD-4c-prereq+e1 — `LinkConfig.strings` materialized as a
    /// rodata `__TEXT,__cstring` region placed after the member
    /// non-text payloads (sharing the same segment) and before
    /// `__TEXT,__stubs`. Empty layout (zero-sized) when
    /// `cfg.strings.is_empty()` so the e0 byte-identical guarantee
    /// for every current caller holds.
    pub user_strings_layout: UserStringsLayout,
    /// SD-4c-prereq+e1 — pre-built byte payload for the user-strings
    /// region. emit_binary splices this directly after the member
    /// non-text payloads; size = `user_strings_layout.total_size`.
    pub user_strings_payload: Vec<u8>,
    /// SD-4c-prereq+e4 — user-binary `__DATA,__bss` placements (per
    /// `LinkConfig.data_globals` entry). vmsize folded into the
    /// __DATA segment; no file payload (zerofill). Empty layout
    /// (`total_vmsize == 0`) when `data_globals.is_empty()`.
    pub user_data_globals_layout: UserDataGlobalsLayout,
    /// SD-4c-prereq+e7 / e8 — user-binary virtual method tables.
    /// e7a placed entries in `__TEXT,__cstring`; e8 migrated the
    /// rodata to a dedicated `__DATA_CONST` segment so dyld can
    /// rebase slots without invalidating the codesigned `__TEXT`
    /// pages. The vtable layout itself (`entries[i].vaddr` /
    /// `file_offset`) is also exposed through
    /// `data_const_layout.vtable_layout`; this field is kept for
    /// the `apply_user_vtable_overrides` + `build_user_vtables_payload`
    /// emit path that needs the layout directly.
    pub user_vtables_layout: UserVtablesLayout,
    /// Fn-name registry Phase 2 Step 3b.3 — `__torajs_fn_name_table[]`
    /// + `__torajs_fn_name_table_count` placement inside `__DATA_CONST`,
    /// after `user_vtables_layout` + class_layouts. Mirror of
    /// `data_const_layout.fn_name_table_layout` for the
    /// `apply_fn_name_table_overrides` / `build_fn_name_table_payload`
    /// emit path that needs the layout directly. Empty layout
    /// (`total_size == 0`) when `cfg.fn_name_globals.is_empty()`.
    pub fn_name_table_layout: FnNameTableLayout,
    /// W-J Phase A3c — `__torajs_class_name_table[]` +
    /// `__torajs_n_class_names` placement inside `__DATA_CONST`,
    /// after `fn_name_table_layout`. Mirror of
    /// `data_const_layout.class_name_table_layout` for the
    /// `apply_class_name_table_overrides` /
    /// `build_class_name_table_payload` emit path. Empty layout
    /// (`total_size == 0`) when `cfg.class_names.is_empty()`.
    pub class_name_table_layout: super::class_name_table_layout::ClassNameTableLayout,
    /// SD-4c-prereq+e8 — `__DATA_CONST` segment placement +
    /// `has_data_const` flag. When `has_data_const` is false the
    /// emit pass skips every `__DATA_CONST` emit hook so the binary
    /// stays byte-identical to the pre-e8 layout.
    pub data_const_layout: super::data_const_layout::DataConstLayout,
    /// SD-4c-prereq+e7b-4/e8-2b — combined __DATA_CONST rebase
    /// chain-link u64 values. First `vtable_rebase_target_count`
    /// entries match `compute_vtable_rebase_targets` output order
    /// (one per `Some` vtable slot); the remainder belong to
    /// `compute_class_layouts_rebase_targets` (one per non-empty
    /// class_layouts entry's inner-ptr slot). The emit pass splits
    /// at `vtable_rebase_target_count` to feed the two `build_*_payload`
    /// helpers their respective slices.
    pub text_rebase_link_values: Vec<u64>,
    /// SD-4c-prereq+e8-2b — split point into `text_rebase_link_values`.
    /// Equals the number of vtable slots that produced a rebase target
    /// (= total `Some` slot_syms across all entries in
    /// `user_vtables_layout`).
    pub vtable_rebase_target_count: usize,
    /// Fn-name registry Phase 2 Step 3b.4 — count of fn_name_table
    /// chain-fixup slots (= `2 × fn_name_globals.len()` for the
    /// `fn_addr` + `name_ptr` slot pair per entry; 0 when
    /// `fn_name_globals.is_empty()`). archive_emit.rs uses this as
    /// the third split point so the 3-way split of
    /// `text_rebase_link_values` lines up:
    /// `vtable_lv[..vtable_rebase_target_count]`,
    /// `class_lv[vtable_rebase_target_count..total - fn_name_rebase_target_count]`,
    /// `fn_name_lv[total - fn_name_rebase_target_count..]`.
    pub fn_name_rebase_target_count: usize,
    /// W-J Phase A3c — count of class_name_table chain-fixup slots
    /// (= `1 × class_names.len()` for the single `name_ptr` slot
    /// per entry; 0 when `class_names.is_empty()`). archive_emit.rs
    /// uses this as the fourth split point so the 4-way split of
    /// `text_rebase_link_values` lines up:
    /// `vtable_lv | class_lv | fn_name_lv | class_name_lv`.
    pub class_name_rebase_target_count: usize,
    /// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-5c.2b — count of
    /// baked-regex chain-fixup slots (= `1 × baked_regex_entries.len()`
    /// for the single `BakedDfaMeta::states_ptr` slot per entry; 0
    /// when `cfg.baked_regex_entries.is_empty()`). archive_emit.rs
    /// uses this as the fifth split point so the 5-way split of
    /// `text_rebase_link_values` lines up:
    /// `vtable_lv | class_lv | fn_name_lv | class_name_lv | baked_regex_lv`.
    pub baked_regex_rebase_target_count: usize,
}

/// Failures `compute_archive_layout` can report.
#[derive(Debug)]
pub enum ArchiveLayoutError {
    /// The opt-in dead-strip pre-pass failed (`TORAJS_LINK_DEADSTRIP`).
    DeadStrip(String),
    /// Top-level archive bytes were malformed.
    Merge(ArchiveMergeError),
    /// User code references externs that no archive defines.
    Link(ArchiveLinkError),
    /// A pulled member's `__TEXT,__text` section couldn't be parsed.
    MemberText {
        archive_idx: usize,
        member_idx: usize,
        err: MemberTextError,
    },
    /// A pulled member's symtab walk failed during defined-extern enumeration.
    MemberSymtab {
        archive_idx: usize,
        member_idx: usize,
        err: MemberSymtabError,
    },
    /// `cfg.entry` doesn't match any user function name.
    EntryNotInUserFuncs { entry: String },
    /// Member-internal reloc decode/patch failed during the S7-C5 pass.
    MemberReloc {
        archive_idx: usize,
        member_idx: usize,
        err: MemberRelocApplyError,
    },
    /// SD-4c-prereq-b2 — `collect_member_sections` rejected a member
    /// during the non-text layout pass.
    MemberSections {
        archive_idx: usize,
        member_idx: usize,
        err: MemberRelocError,
    },
    /// SD-4c-prereq+b2 — TLV sym vaddr override pass rejected a
    /// member. Wraps the underlying [`TlvSymOverrideError`].
    TlvOverride(TlvSymOverrideError),
    /// SD-4c-prereq+e4 — `LinkConfig.data_globals` is non-empty but
    /// the worklist closure didn't surface any dyld imports, so no
    /// `__DATA` segment exists to host `__DATA,__bss`. Production
    /// codegen always emits libSystem references alongside any
    /// data globals (rc_inc/dec, str_alloc, etc.) so this should be
    /// unreachable; the explicit error guards against silent-wrong
    /// when a synthetic test path skips the dyld layer.
    DataGlobalsWithoutDyld { count: usize },
}

impl From<TlvSymOverrideError> for ArchiveLayoutError {
    fn from(err: TlvSymOverrideError) -> Self {
        ArchiveLayoutError::TlvOverride(err)
    }
}
