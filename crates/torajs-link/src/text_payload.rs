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
//!
//! A loud-reject stub (a judged-away entry that only tail-branches
//! into the reject landing pad) never runs in a correctly judged
//! program and has no timing to keep, so it packs on instruction
//! alignment; the next function still opens a fetch line.

use torajs_codegen::CompiledFunction;
use torajs_codegen::reloc::{CallTarget, RelocKind};

use crate::archive_emit::pad_to;
use crate::archive_link::ArchiveLayout;
use crate::resolve::ResolvedFunction;

/// One Apple Silicon fetch line.
pub const TEXT_FN_ALIGN: u32 = 64;

/// Mach-O names of the reject landing pads share this prefix
/// (`torajs-dispatch`: `__torajs_dispatch_stub_reject` and its
/// pointer-answering twin).
const REJECT_PAD_PREFIX: &str = "___torajs_dispatch_stub_reject";

/// `f`'s placement: its byte length, and whether it is a reject stub
/// (`movz x7, #fam` + `b <pad>`), which packs.
pub(crate) fn text_slot(f: &CompiledFunction) -> (u32, bool) {
    let stub = f.bytes.len() == 8
        && matches!(
            f.relocs.as_slice(),
            [r] if r.byte_offset == 4
                && matches!(&r.kind, RelocKind::CallSite { target: CallTarget::Extern(pad) }
                    if pad.starts_with(REJECT_PAD_PREFIX))
        );
    (f.bytes.len() as u32, stub)
}

/// Offsets relative to the start of `__text`.
pub(crate) struct TextPlacement {
    pub(crate) user: Vec<u32>,
    pub(crate) members: Vec<u32>,
    /// End of the last placed byte (no trailing pad).
    pub(crate) size: u32,
}

/// `user_slots` are [`text_slot`]s, in function order.
pub(crate) fn place_text(
    user_slots: impl Iterator<Item = (u32, bool)>,
    member_sizes: &[u32],
) -> TextPlacement {
    let mut cursor: u32 = 0;
    let mut place = |(size, packed): (u32, bool)| {
        let at = cursor.next_multiple_of(if packed { 4 } else { TEXT_FN_ALIGN });
        cursor = at + size;
        at
    };
    let user = user_slots.map(&mut place).collect();
    let members = member_sizes.iter().map(|&s| place((s, false))).collect();
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
        let p = place_text([4, 72, 64].map(|s| (s, false)).into_iter(), &[8, 100]);
        assert_eq!(p.user, vec![0, 64, 192]);
        assert_eq!(p.members, vec![256, 320]);
        assert_eq!(p.size, 420);
    }

    /// Reject stubs pack on 4 bytes; whatever follows them still opens
    /// a fetch line.
    #[test]
    fn reject_stubs_pack_and_the_next_function_realigns() {
        let p = place_text(
            [(72, false), (8, true), (8, true), (12, false)].into_iter(),
            &[8],
        );
        assert_eq!(p.user, vec![0, 72, 80, 128]);
        assert_eq!(p.members, vec![192]);
        assert_eq!(p.size, 200);
    }

    fn fn_with(bytes: usize, target: &str, at: u32) -> CompiledFunction {
        CompiledFunction {
            name: "___torajs_dispatch_str_arm".into(),
            bytes: vec![0; bytes],
            relocs: vec![torajs_codegen::reloc::Reloc {
                byte_offset: at,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern(target.into()),
                },
            }],
            frame: torajs_codegen::frame::FrameLayout::leaf_no_spill(),
        }
    }

    /// Only the exact stub shape packs: eight bytes, one tail branch
    /// into a reject landing pad.
    #[test]
    fn only_a_reject_stub_packs() {
        let pad = "___torajs_dispatch_stub_reject";
        assert_eq!(text_slot(&fn_with(8, pad, 4)), (8, true));
        assert_eq!(
            text_slot(&fn_with(8, "___torajs_dispatch_stub_reject_ptr", 4)),
            (8, true)
        );
        assert_eq!(text_slot(&fn_with(12, pad, 8)), (12, false));
        assert_eq!(text_slot(&fn_with(8, "___torajs_rc_dec", 4)), (8, false));
        assert_eq!(text_slot(&fn_with(8, pad, 0)), (8, false));
    }

    #[test]
    fn empty_layout_has_zero_size() {
        let p = place_text(std::iter::empty(), &[]);
        assert!(p.user.is_empty() && p.members.is_empty());
        assert_eq!(p.size, 0);
    }
}
