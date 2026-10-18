//! Member payloads for the archive-aware emit: each laid-out
//! member's `__text` patched in place against the effective symbol
//! table, and its other `__TEXT,*` sections sliced out unchanged.

use crate::archive_emit::member_data_layouts;
use crate::archive_link::{ArchiveLayout, ArchiveLayoutError};
use crate::archives_merge::MergedArchives;
use crate::member_apply::apply_member_relocs;
use crate::resolve::SymTable;

/// Each member's `__text` bytes, and its non-text `__TEXT` section bytes.
type MemberPayloads = (Vec<Vec<u8>>, Vec<Vec<u8>>);

/// The patched `__text` of every laid-out member, in layout order,
/// and the bytes of its non-text `__TEXT` sections, in
/// `compute_non_text_layouts` order.
pub(crate) fn member_text_payloads(
    layout: &ArchiveLayout,
    merged: &MergedArchives<'_>,
    sym_table: &SymTable,
) -> Result<MemberPayloads, ArchiveLayoutError> {
    let mut text: Vec<Vec<u8>> = Vec::with_capacity(layout.member_layouts.len());
    let mut non_text: Vec<Vec<u8>> = Vec::new();
    for (mi, m) in layout.member_layouts.iter().enumerate() {
        let member = &merged.member(m.key.0, m.key.1);
        let off = m.member_text_offset_in_member as usize;
        let end = off + m.text_size as usize;
        let mut bytes = member.data[off..end].to_vec();
        apply_member_relocs(
            &mut bytes,
            member,
            m.vaddr,
            sym_table,
            &m.non_text_sections,
            member_data_layouts(layout, mi),
        )
        .map_err(|err| ArchiveLayoutError::MemberReloc {
            archive_idx: m.key.0,
            member_idx: m.key.1,
            err,
        })?;
        text.push(bytes);
        for s in &m.non_text_sections {
            let off = s.member_internal_offset as usize;
            let end = off + s.size as usize;
            non_text.push(member.data[off..end].to_vec());
        }
    }
    Ok((text, non_text))
}
