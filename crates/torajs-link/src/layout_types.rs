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
use crate::member_apply::MemberRelocApplyError;
use crate::member_reloc::MemberRelocError;
use crate::member_text::MemberTextError;
use crate::non_text_layout::NonTextSectionLayout;

/// Per-archive-member computed data the S7-C4 emit pass needs.
/// `defined_syms` carries `(name, n_value)` exactly as the member's
/// `LC_SYMTAB` records them — `n_value` is section-relative inside
/// the member's `__text`.
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
    pub defined_syms: Vec<(String, u64)>,
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
    pub stubs_section_vaddr: u64,
    pub stubs_section_size: u64,
    pub stubs_file_offset: u32,
    pub la_ptr_section_vaddr: u64,
    pub la_ptr_section_size: u64,
    pub la_ptr_file_offset: u32,
    pub data_vmsize: u64,
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
}

/// Failures `compute_archive_layout` can report.
#[derive(Debug)]
pub enum ArchiveLayoutError {
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
}
