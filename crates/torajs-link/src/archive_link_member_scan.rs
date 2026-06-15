//! Phase 2 of [`compute_archive_layout`] — scan each required `.o`
//! member's `__text` size/offset + defined externs into parallel
//! `Vec`s indexed by `member_keys` order.

use crate::archive::parse_member_defined_externs;
use crate::archives_merge::MergedArchives;
use crate::layout_types::ArchiveLayoutError;
use crate::member_text::parse_member_text_section;

pub struct MemberScanLayouts {
    pub text_sizes: Vec<u32>,
    pub text_offsets: Vec<u32>,
    pub defined_syms: Vec<Vec<(String, u64, u8)>>,
}

pub fn scan_member_text_and_symbols(
    merged: &MergedArchives<'_>,
    member_keys: &[(usize, usize)],
) -> Result<MemberScanLayouts, ArchiveLayoutError> {
    let n = member_keys.len();
    let mut text_sizes: Vec<u32> = Vec::with_capacity(n);
    let mut text_offsets: Vec<u32> = Vec::with_capacity(n);
    let mut defined_syms: Vec<Vec<(String, u64, u8)>> = Vec::with_capacity(n);
    for &(a_idx, m_idx) in member_keys {
        let member = &merged.per_archive_members[a_idx][m_idx];
        let (text_off, text_size) =
            parse_member_text_section(member).map_err(|err| ArchiveLayoutError::MemberText {
                archive_idx: a_idx,
                member_idx: m_idx,
                err,
            })?;
        text_offsets.push(text_off);
        text_sizes.push(text_size);
        let defs = parse_member_defined_externs(member).map_err(|err| {
            ArchiveLayoutError::MemberSymtab {
                archive_idx: a_idx,
                member_idx: m_idx,
                err,
            }
        })?;
        defined_syms.push(defs);
    }
    Ok(MemberScanLayouts {
        text_sizes,
        text_offsets,
        defined_syms,
    })
}
