//! Member payloads for the archive-aware emit: each laid-out
//! member's `__text` patched in place against the effective symbol
//! table, and its other `__TEXT,*` sections sliced out unchanged.

use crate::archive_emit::{member_data_layouts, pad_to};
use crate::archive_link::{ArchiveLayout, ArchiveLayoutError};
use crate::archives_merge::MergedArchives;
use crate::member_apply::{MemberRelocApplyError, apply_member_relocs};
use crate::member_reloc::MemberRelocError;
use crate::resolve::SymLookup;

/// What writes the laid-out members' bytes into the output: the
/// members, the effective symbol table their `__text` relocs resolve
/// against, and the chained-fixup link values of their `__DATA` slots.
pub(crate) struct MemberText<'m, 'a> {
    pub(crate) merged: &'m MergedArchives<'a>,
    pub(crate) sym_table: &'m dyn SymLookup,
    pub(crate) data_link_values: &'m [u64],
}

impl MemberText<'_, '_> {
    /// Append the `__text` of member `mi` of `layout` to `buf`, patched
    /// in place against the symbol table.
    pub(crate) fn write_text(
        &self,
        buf: &mut Vec<u8>,
        layout: &ArchiveLayout,
        mi: usize,
    ) -> Result<(), ArchiveLayoutError> {
        let m = &layout.member_layouts[mi];
        let reloc_err = |err| ArchiveLayoutError::MemberReloc {
            archive_idx: m.key.0,
            member_idx: m.key.1,
            err,
        };
        let obj = self.merged.obj(m.key.0, m.key.1);
        let obj = obj.map_err(|e| reloc_err(MemberRelocApplyError::Decode(e)))?;
        let text = obj.text_section().ok_or_else(|| {
            reloc_err(MemberRelocApplyError::Decode(
                MemberRelocError::NoTextSection,
            ))
        })?;
        let start = buf.len();
        obj.payload_into(text.section_index, buf);
        apply_member_relocs(
            &mut buf[start..],
            &obj,
            m.vaddr,
            self.sym_table,
            &m.non_text_sections,
            member_data_layouts(layout, mi),
        )
        .map_err(reloc_err)
    }

    /// Emit each non-text section payload at its layout-assigned
    /// `final_file_offset`, padding with zeros to reconcile per-section
    /// `section_64.align` boundaries that `compute_non_text_layouts`
    /// honored. Called from `archive_emit::emit_binary` between the
    /// patched member-text payloads and `__TEXT,__stubs`.
    ///
    /// swap-2k chunk 3 — without the per-section `pad_to`, alignment
    /// padding budgeted into `final_file_offset` would not appear in the
    /// emitted byte stream and `__TEXT,__literal16` would land at the
    /// non-padded cumulative offset, mismatching the patcher's
    /// `final_vaddr` reloc target after `patch_pageoff12`'s silent
    /// `(offset >> scale) << scale` round-trip.
    pub(crate) fn write_non_text(
        &self,
        buf: &mut Vec<u8>,
        layout: &ArchiveLayout,
    ) -> Result<(), ArchiveLayoutError> {
        for m in &layout.member_layouts {
            let obj = self.merged.obj(m.key.0, m.key.1);
            let obj = obj.map_err(|e| ArchiveLayoutError::MemberReloc {
                archive_idx: m.key.0,
                member_idx: m.key.1,
                err: MemberRelocApplyError::Decode(e),
            })?;
            for s in &m.non_text_sections {
                pad_to(buf, s.final_file_offset as usize);
                obj.payload_into(s.section_index, buf);
            }
        }
        Ok(())
    }
}
