//! `__TEXT,__text` parser for archive members.
//!
//! Extracted out of `archive_link.rs` so that file can stay under
//! the 500-prod-LOC hard limit after SD-2a added the dyld stub /
//! lazy pointer layout fields. The S7-C3 logic is unchanged; only
//! the file boundary moved.
//!
//! Object-file Mach-O conventionally puts every section under one
//! anonymous `LC_SEGMENT_64` (empty `segname`); the final-binary
//! layout splits sections across named segments. The identifying
//! tuple is per-section `(sectname, segname)` — so we walk every
//! `LC_SEGMENT_64` and verify `__TEXT,__text` at section level
//! rather than relying on the segment name.

use torajs_obj::{LC_SEGMENT_64, MH_MAGIC_64, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE};

use crate::archive::ArMember;

const TEXT_SEGNAME: &[u8] = b"__TEXT";
const TEXT_SECTNAME: &[u8] = b"__text";

/// Failures `parse_member_text_section` can report.
#[derive(Debug, PartialEq, Eq)]
pub enum MemberTextError {
    /// Member is too small to be a Mach-O 64.
    TruncatedHeader,
    /// A load command's cmdsize would extend past the buffer or
    /// is smaller than the minimum 8-byte header.
    TruncatedLoadCommand { offset: usize },
    /// `LC_SEGMENT_64` advertised more sections than fit.
    TruncatedSegmentSections { offset: usize },
    /// No `__TEXT,__text` section was found. A `.o` codegen-emit
    /// won't ship without one; if it happens the member isn't a
    /// torajs-obj output.
    NoTextSection,
}

/// Parse a `.o` member's `__TEXT,__text` section. Returns
/// `(text_offset_in_member, text_size)` where `text_offset_in_member`
/// is the byte offset inside the member where the payload starts
/// (= the `section_64.offset` field) and `text_size` is the
/// payload length (= the `section_64.size` field).
///
/// The caller pairs `text_offset_in_member` with
/// `ArMember.data[text_offset_in_member..+ text_size]` to slice
/// out the member's `__text` payload bytes.
pub fn parse_member_text_section(member: &ArMember<'_>) -> Result<(u32, u32), MemberTextError> {
    let bytes = member.data;
    if bytes.len() < 32 {
        return Err(MemberTextError::TruncatedHeader);
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != MH_MAGIC_64 {
        return Err(MemberTextError::TruncatedHeader);
    }
    let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;

    let mut cursor = 32usize;
    for _ in 0..ncmds {
        if bytes.len() < cursor + 8 {
            return Err(MemberTextError::TruncatedLoadCommand { offset: cursor });
        }
        let cmd = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        let cmdsize =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        if cmdsize < 8 || bytes.len() < cursor + cmdsize {
            return Err(MemberTextError::TruncatedLoadCommand { offset: cursor });
        }
        if cmd == LC_SEGMENT_64 {
            if cmdsize < SEGMENT_COMMAND_64_SIZE as usize {
                return Err(MemberTextError::TruncatedLoadCommand { offset: cursor });
            }
            let nsects =
                u32::from_le_bytes(bytes[cursor + 64..cursor + 68].try_into().unwrap()) as usize;
            let sections_start = cursor + SEGMENT_COMMAND_64_SIZE as usize;
            let need = sections_start + nsects * SECTION_64_SIZE as usize;
            if bytes.len() < need || cmdsize < need - cursor {
                return Err(MemberTextError::TruncatedSegmentSections { offset: cursor });
            }
            for i in 0..nsects {
                let sec = sections_start + i * SECTION_64_SIZE as usize;
                if !name16_eq(&bytes[sec..sec + 16], TEXT_SECTNAME) {
                    continue;
                }
                if !name16_eq(&bytes[sec + 16..sec + 32], TEXT_SEGNAME) {
                    continue;
                }
                let size = u64::from_le_bytes(bytes[sec + 40..sec + 48].try_into().unwrap());
                let offset = u32::from_le_bytes(bytes[sec + 48..sec + 52].try_into().unwrap());
                return Ok((offset, size as u32));
            }
        }
        cursor += cmdsize;
    }
    Err(MemberTextError::NoTextSection)
}

/// 16-byte name-field comparison: returns true iff `slot`'s
/// leading bytes (up to NUL / space / end-of-slot) match `name`.
fn name16_eq(slot: &[u8], name: &[u8]) -> bool {
    if slot.len() != 16 {
        return false;
    }
    let mut end = slot.len();
    while end > 0 && (slot[end - 1] == 0 || slot[end - 1] == b' ') {
        end -= 1;
    }
    &slot[..end] == name
}
