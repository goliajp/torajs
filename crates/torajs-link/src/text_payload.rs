//! The `__text` payload of an archive-linked executable: user
//! functions (relocations resolved), then each integrated member's
//! patched `__text` slice.

use crate::archive_link::ArchiveLayout;
use crate::resolve::ResolvedFunction;

pub(crate) fn write_text_payload(
    buf: &mut Vec<u8>,
    layout: &ArchiveLayout,
    resolved: &[ResolvedFunction],
    member_text_payloads: &[Vec<u8>],
) {
    for r in resolved {
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
        buf.extend_from_slice(&member_text_payloads[i]);
    }
}
