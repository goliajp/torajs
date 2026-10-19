//! Phase 2 of [`compute_archive_layout`] — scan each required `.o`
//! member's `__text` size/offset + defined externs into parallel
//! `Vec`s indexed by `member_keys` order.

use crate::archives_merge::MergedArchives;
use crate::layout_types::ArchiveLayoutError;
use crate::member_reloc::MemberRelocError;
use crate::member_text::MemberTextError;

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
        let text_err = |err| ArchiveLayoutError::MemberText {
            archive_idx: a_idx,
            member_idx: m_idx,
            err,
        };
        let obj = merged
            .obj(a_idx, m_idx)
            .map_err(|e| text_err(text_error(e)))?;
        let text = obj
            .text_section()
            .ok_or_else(|| text_err(MemberTextError::NoTextSection))?;
        let (text_off, text_size) = (text.member_internal_offset, text.size);
        text_offsets.push(text_off);
        text_sizes.push(text_size);
        let defs = obj
            .defined_externs()
            .map_err(|err| ArchiveLayoutError::MemberSymtab {
                archive_idx: a_idx,
                member_idx: m_idx,
                err,
            })?;
        // Layout keeps owned names (MemberLayout has no lifetime);
        // only the required members' syms materialize here — small.
        defined_syms.push(
            defs.into_iter()
                .map(|(n, v, s)| (n.to_string(), v, s))
                .collect(),
        );
    }
    Ok(MemberScanLayouts {
        text_sizes,
        text_offsets,
        defined_syms,
    })
}

/// A load-command walk failure in [`MemberTextError`] terms.
fn text_error(e: MemberRelocError) -> MemberTextError {
    match e {
        MemberRelocError::TruncatedLoadCommand { offset } => {
            MemberTextError::TruncatedLoadCommand { offset }
        }
        MemberRelocError::TruncatedSegmentSections { offset } => {
            MemberTextError::TruncatedSegmentSections { offset }
        }
        _ => MemberTextError::TruncatedHeader,
    }
}
