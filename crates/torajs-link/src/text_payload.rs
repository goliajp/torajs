//! The `__text` payload of an archive-linked executable: user
//! functions (relocations resolved), then each integrated member's
//! patched `__text` slice.
//!
//! Every user function and every member starts on a
//! [`TEXT_FN_ALIGN`] boundary. Packed back to back, a function's
//! offset within a fetch line depended on the byte length of every
//! function laid out before it, so a change to one function moved the
//! code of its neighbours and their timings with it. The gaps are
//! zero-filled (`udf #0`): nothing branches into them.

use crate::archive_emit::pad_to;
use crate::archive_link::ArchiveLayout;
use crate::resolve::ResolvedFunction;

/// One Apple Silicon fetch line.
pub const TEXT_FN_ALIGN: u32 = 64;

/// Offsets relative to the start of `__text`.
pub(crate) struct TextPlacement {
    pub(crate) user: Vec<u32>,
    pub(crate) members: Vec<u32>,
    /// End of the last placed byte (no trailing pad).
    pub(crate) size: u32,
}

pub(crate) fn place_text(
    user_sizes: impl Iterator<Item = u32>,
    member_sizes: &[u32],
) -> TextPlacement {
    let mut cursor: u32 = 0;
    let mut place = |size: u32| {
        let at = cursor.next_multiple_of(TEXT_FN_ALIGN);
        cursor = at + size;
        at
    };
    let user = user_sizes.map(&mut place).collect();
    let members = member_sizes.iter().map(|&s| place(s)).collect();
    TextPlacement {
        user,
        members,
        size: cursor,
    }
}

pub(crate) fn write_text_payload(
    buf: &mut Vec<u8>,
    layout: &ArchiveLayout,
    resolved: &[ResolvedFunction],
    member_text_payloads: &[Vec<u8>],
) {
    let text_vmaddr = layout.text_vmaddr;
    for (r, &vaddr) in resolved.iter().zip(&layout.fn_vaddrs) {
        pad_to(buf, (vaddr - text_vmaddr) as usize);
        buf.extend_from_slice(&r.bytes);
    }
    debug_assert_eq!(
        member_text_payloads.len(),
        layout.member_layouts.len(),
        "patched payload count must match member_layouts",
    );
    for (i, m) in layout.member_layouts.iter().enumerate() {
        debug_assert_eq!(
            member_text_payloads[i].len() as u32,
            m.text_size,
            "patched member __text size must match layout",
        );
        pad_to(buf, m.file_offset as usize);
        buf.extend_from_slice(&member_text_payloads[i]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_start_is_aligned_and_nothing_overlaps() {
        let p = place_text([4, 72, 64].into_iter(), &[8, 100]);
        assert_eq!(p.user, vec![0, 64, 192]);
        assert_eq!(p.members, vec![256, 320]);
        assert_eq!(p.size, 420);
    }

    #[test]
    fn empty_layout_has_zero_size() {
        let p = place_text(std::iter::empty(), &[]);
        assert!(p.user.is_empty() && p.members.is_empty());
        assert_eq!(p.size, 0);
    }
}
