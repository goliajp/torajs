//! Member-internal reloc decoder — S7-C5a.
//!
//! S7-C4 integrated archive members' `__TEXT,__text` bytes verbatim;
//! that worked for leaf-only members but real `libtorajs_*.a` members
//! BL each other (member X's `__text` references member Y's
//! defined-extern). S7-C5a parses each member's reloc table; the
//! patcher (`member_apply::apply_member_relocs`, S7-C5b) consumes
//! the decoded entries; the emit pass (`archive_emit`, S7-C5c) wires
//! both into the final-link pipeline.
//!
//! Wire format mirrors `<mach-o/reloc.h>` `relocation_info`:
//!
//! ```text
//!   struct relocation_info {              // 8 bytes
//!       int32_t  r_address;               // section-relative byte offset
//!       uint32_t r_symbolnum : 24;        // bits  0..23
//!       uint32_t r_pcrel     : 1;         // bit   24
//!       uint32_t r_length    : 2;         // bits 25..26
//!       uint32_t r_extern    : 1;         // bit   27
//!       uint32_t r_type      : 4;         // bits 28..31
//!   };
//! ```

use torajs_obj::{
    LC_SEGMENT_64, MH_MAGIC_64, RELOCATION_INFO_SIZE, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
};

use crate::archive::ArMember;

/// One decoded `relocation_info` entry from a member's
/// `__TEXT,__text` reloff/nreloc table. Field names mirror
/// `<mach-o/reloc.h>` so the bit-unpack stays auditable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemberRelocEntry {
    /// Byte offset *within the member's `__text` payload* — pair
    /// with `member_vaddr + r_address` to get the final patch site
    /// vaddr.
    pub r_address: u32,
    /// For `r_extern=1`: index into the member's `LC_SYMTAB` nlist
    /// — that entry's `n_strx` resolves the symbol name.
    /// For `r_extern=0`: 1-based section index — section-keyed
    /// relocs (rustc / cc can emit these for local jumps).
    pub r_symbolnum: u32,
    pub r_pcrel: u8,
    pub r_length: u8,
    pub r_extern: u8,
    pub r_type: u8,
}

/// Failures `parse_member_text_relocs` can surface. All variants
/// surface the offending offset / count so the link driver can
/// blame a specific archive member.
#[derive(Debug, PartialEq, Eq)]
pub enum MemberRelocError {
    /// Member is smaller than a Mach-O 64 header.
    TruncatedHeader,
    /// A load command's `cmdsize` would extend past the buffer or
    /// is below the 8-byte minimum.
    TruncatedLoadCommand { offset: usize },
    /// `LC_SEGMENT_64` advertised more sections than fit in its
    /// `cmdsize`.
    TruncatedSegmentSections { offset: usize },
    /// Section header advertised a reloff/nreloc range past the
    /// member bytes.
    TruncatedRelocs {
        reloff: u32,
        nreloc: u32,
        file_size: usize,
    },
    /// No `__TEXT,__text` section was found. Members emitted by
    /// `torajs_obj::write_object` always have one; a missing
    /// section means the member isn't a valid `.o`.
    NoTextSection,
}

/// Mach-O segment / section name constants — same values as
/// `archive_link.rs` uses for `parse_member_text_section`. Kept
/// local to this module to avoid pulling sibling-private items.
const TEXT_SEGNAME: &[u8] = b"__TEXT";
const TEXT_SECTNAME: &[u8] = b"__text";

/// Walk a member's Mach-O headers, find `__TEXT,__text`, and decode
/// every entry in its `reloff..reloff + nreloc * 8` range. Returns
/// the entries in on-disk order (which is the order codegen emitted
/// them — typically lex-sorted by site offset).
///
/// `nreloc == 0` returns an empty `Vec` — that's the common case
/// for leaf-only members (e.g. S7-C4's `_foo: ret 7` fixture).
pub fn parse_member_text_relocs(
    member: &ArMember<'_>,
) -> Result<Vec<MemberRelocEntry>, MemberRelocError> {
    let bytes = member.data;
    if bytes.len() < 32 {
        return Err(MemberRelocError::TruncatedHeader);
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != MH_MAGIC_64 {
        return Err(MemberRelocError::TruncatedHeader);
    }
    let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;

    // Object-file Mach-O conventionally puts every section under one
    // anonymous LC_SEGMENT_64 (empty segname); final binaries split
    // sections across named segments (__TEXT / __DATA / …). The
    // identifying tuple is *per-section* (sectname, segname), not
    // the segment header's segname — so walk every LC_SEGMENT_64
    // and verify __TEXT,__text at section level.
    let mut cursor = 32usize;
    for _ in 0..ncmds {
        if bytes.len() < cursor + 8 {
            return Err(MemberRelocError::TruncatedLoadCommand { offset: cursor });
        }
        let cmd = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        let cmdsize =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        if cmdsize < 8 || bytes.len() < cursor + cmdsize {
            return Err(MemberRelocError::TruncatedLoadCommand { offset: cursor });
        }
        if cmd != LC_SEGMENT_64 {
            cursor += cmdsize;
            continue;
        }
        if cmdsize < SEGMENT_COMMAND_64_SIZE as usize {
            return Err(MemberRelocError::TruncatedLoadCommand { offset: cursor });
        }
        let nsects =
            u32::from_le_bytes(bytes[cursor + 64..cursor + 68].try_into().unwrap()) as usize;
        let sections_start = cursor + SEGMENT_COMMAND_64_SIZE as usize;
        let need = sections_start + nsects * SECTION_64_SIZE as usize;
        if bytes.len() < need || cmdsize < need - cursor {
            return Err(MemberRelocError::TruncatedSegmentSections { offset: cursor });
        }
        for i in 0..nsects {
            let sec = sections_start + i * SECTION_64_SIZE as usize;
            if !name16_eq(&bytes[sec..sec + 16], TEXT_SECTNAME) {
                continue;
            }
            if !name16_eq(&bytes[sec + 16..sec + 32], TEXT_SEGNAME) {
                continue;
            }
            let reloff = u32::from_le_bytes(bytes[sec + 56..sec + 60].try_into().unwrap());
            let nreloc = u32::from_le_bytes(bytes[sec + 60..sec + 64].try_into().unwrap());
            return decode_reloc_table(bytes, reloff, nreloc);
        }
        cursor += cmdsize;
    }
    Err(MemberRelocError::NoTextSection)
}

/// Decode `nreloc` consecutive 8-byte `relocation_info` entries
/// starting at file offset `reloff`. Pure bit-unpack; mirrors the
/// inverse of `RelocationInfo::write_to` in `torajs-obj`.
pub(crate) fn decode_reloc_table(
    bytes: &[u8],
    reloff: u32,
    nreloc: u32,
) -> Result<Vec<MemberRelocEntry>, MemberRelocError> {
    if nreloc == 0 {
        return Ok(Vec::new());
    }
    let start = reloff as usize;
    let total = (nreloc as usize)
        .checked_mul(RELOCATION_INFO_SIZE as usize)
        .ok_or(MemberRelocError::TruncatedRelocs {
            reloff,
            nreloc,
            file_size: bytes.len(),
        })?;
    let end = start
        .checked_add(total)
        .ok_or(MemberRelocError::TruncatedRelocs {
            reloff,
            nreloc,
            file_size: bytes.len(),
        })?;
    if end > bytes.len() {
        return Err(MemberRelocError::TruncatedRelocs {
            reloff,
            nreloc,
            file_size: bytes.len(),
        });
    }

    let mut out: Vec<MemberRelocEntry> = Vec::with_capacity(nreloc as usize);
    for i in 0..nreloc as usize {
        let off = start + i * RELOCATION_INFO_SIZE as usize;
        let r_address = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
        let packed = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap());
        out.push(MemberRelocEntry {
            r_address,
            r_symbolnum: packed & 0x00FF_FFFF,
            r_pcrel: ((packed >> 24) & 0b1) as u8,
            r_length: ((packed >> 25) & 0b11) as u8,
            r_extern: ((packed >> 27) & 0b1) as u8,
            r_type: ((packed >> 28) & 0b1111) as u8,
        });
    }
    Ok(out)
}

/// 16-byte name-field comparison — returns true iff the slot's
/// leading bytes (up to NUL / space / end-of-slot) match `name`.
/// Same logic as `archive_link::name16_eq`; duplicated to keep
/// modules independent.
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

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;
    use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};
    use torajs_obj::{ARM64_RELOC_BRANCH26, ARM64_RELOC_PAGE21, write_object};

    const BL_PLACEHOLDER: [u8; 4] = [0x00, 0x00, 0x00, 0x94];
    const RET_BYTES: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];

    fn build_short_name_archive(name: &str, data: &[u8]) -> Vec<u8> {
        use crate::archive::{AR_HEADER_SIZE, AR_MAGIC};
        assert!(name.len() <= 16);
        let mut archive: Vec<u8> = Vec::new();
        archive.extend_from_slice(AR_MAGIC);
        let mut header = [b' '; AR_HEADER_SIZE];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[16] = b'0';
        let size_str = format!("{}", data.len());
        header[48..48 + size_str.len()].copy_from_slice(size_str.as_bytes());
        header[58] = b'`';
        header[59] = b'\n';
        archive.extend_from_slice(&header);
        archive.extend_from_slice(data);
        if data.len() % 2 != 0 {
            archive.push(0);
        }
        archive
    }

    /// `fn _foo() { _bar(); ret }` — one BRANCH26 reloc targeting
    /// the extern `_bar`. Body: BL placeholder + RET = 8 bytes,
    /// reloc at byte_offset 0.
    fn foo_calls_bar() -> CompiledFunction {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&BL_PLACEHOLDER);
        bytes.extend_from_slice(&RET_BYTES);
        CompiledFunction {
            name: "_foo".into(),
            bytes,
            relocs: vec![Reloc {
                byte_offset: 0,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern("_bar".into()),
                },
            }],
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// Leaf fn — no relocs.
    fn fn_leaf(name: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: RET_BYTES.to_vec(),
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    #[test]
    fn leaf_member_has_zero_relocs() {
        let foo = fn_leaf("_foo");
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = crate::archive::parse_archive(&archive).unwrap();
        let entries = parse_member_text_relocs(&members[0]).unwrap();
        assert!(entries.is_empty(), "leaf member has no relocs");
    }

    #[test]
    fn extern_branch26_reloc_decoded() {
        let foo = foo_calls_bar();
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = crate::archive::parse_archive(&archive).unwrap();
        let entries = parse_member_text_relocs(&members[0]).unwrap();

        assert_eq!(entries.len(), 1, "one BRANCH26 reloc expected");
        let r = entries[0];
        assert_eq!(r.r_address, 0, "BL site at byte 0");
        assert_eq!(r.r_pcrel, 1, "BRANCH26 is PC-relative");
        assert_eq!(r.r_length, 2, "BRANCH26 patches a 4-byte word");
        assert_eq!(r.r_extern, 1, "torajs-obj emits symbol-keyed relocs");
        assert_eq!(r.r_type, ARM64_RELOC_BRANCH26);
    }

    /// Multi-reloc member: two BLs at distinct offsets. Verifies
    /// the decoder walks the full `nreloc` range and yields entries
    /// in on-disk order.
    #[test]
    fn multi_reloc_decoded_in_order() {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&BL_PLACEHOLDER);
        bytes.extend_from_slice(&BL_PLACEHOLDER);
        bytes.extend_from_slice(&RET_BYTES);
        let foo = CompiledFunction {
            name: "_foo".into(),
            bytes,
            relocs: vec![
                Reloc {
                    byte_offset: 0,
                    kind: RelocKind::CallSite {
                        target: CallTarget::Extern("_bar".into()),
                    },
                },
                Reloc {
                    byte_offset: 4,
                    kind: RelocKind::CallSite {
                        target: CallTarget::Extern("_baz".into()),
                    },
                },
            ],
            frame: FrameLayout::leaf_no_spill(),
        };
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = crate::archive::parse_archive(&archive).unwrap();
        let entries = parse_member_text_relocs(&members[0]).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].r_address, 0);
        assert_eq!(entries[1].r_address, 4);
        assert_eq!(entries[0].r_type, ARM64_RELOC_BRANCH26);
        assert_eq!(entries[1].r_type, ARM64_RELOC_BRANCH26);
    }

    /// Truncated member (< 32 bytes Mach-O header) surfaces as a
    /// typed error.
    #[test]
    fn truncated_header_returns_typed_error() {
        let too_small = ArMember {
            name: "tiny.o",
            data: &[0u8; 10],
        };
        assert_eq!(
            parse_member_text_relocs(&too_small).unwrap_err(),
            MemberRelocError::TruncatedHeader,
        );
    }

    /// Non-Mach-O magic → TruncatedHeader (the parser doesn't try
    /// to interpret a fat / 32-bit / non-Mach-O blob).
    #[test]
    fn wrong_magic_returns_truncated_header() {
        let mut data = [0u8; 64];
        data[0..4].copy_from_slice(&0xDEAD_BEEF_u32.to_le_bytes());
        let m = ArMember {
            name: "bogus.o",
            data: &data,
        };
        assert_eq!(
            parse_member_text_relocs(&m).unwrap_err(),
            MemberRelocError::TruncatedHeader,
        );
    }

    /// Bit-pack round-trip — encode a `RelocationInfo::page21` via
    /// torajs-obj's writer, feed the bytes through the decoder, and
    /// verify every field landed back in the right slot.
    #[test]
    fn page21_bit_unpack_matches_encoder() {
        use torajs_obj::RelocationInfo;
        let info = RelocationInfo::page21(0x40, 5);
        let mut buf: Vec<u8> = Vec::new();
        info.write_to(&mut buf);
        assert_eq!(buf.len(), RELOCATION_INFO_SIZE as usize);

        // Hand-construct a 32-byte mock member whose only content
        // is the encoded reloc. The decoder reads the bytes directly
        // out of `bytes[reloff..]` so we don't need a real Mach-O
        // wrapper — we call `decode_reloc_table` directly.
        let mut bytes = vec![0u8; 8];
        bytes.copy_from_slice(&buf);
        let entries = decode_reloc_table(&bytes, 0, 1).unwrap();
        assert_eq!(entries.len(), 1);
        let r = entries[0];
        assert_eq!(r.r_address, 0x40);
        assert_eq!(r.r_symbolnum, 5);
        assert_eq!(r.r_pcrel, 1);
        assert_eq!(r.r_length, 2);
        assert_eq!(r.r_extern, 1);
        assert_eq!(r.r_type, ARM64_RELOC_PAGE21);
    }

    /// nreloc claims more entries than fit in the buffer →
    /// TruncatedRelocs.
    #[test]
    fn truncated_relocs_returns_typed_error() {
        let bytes = vec![0u8; 4]; // less than one reloc entry
        let err = decode_reloc_table(&bytes, 0, 1).unwrap_err();
        match err {
            MemberRelocError::TruncatedRelocs {
                reloff,
                nreloc,
                file_size,
            } => {
                assert_eq!(reloff, 0);
                assert_eq!(nreloc, 1);
                assert_eq!(file_size, 4);
            }
            other => panic!("expected TruncatedRelocs, got {other:?}"),
        }
    }

    /// Zero reloc count short-circuits — no walk, empty Vec.
    #[test]
    fn zero_nreloc_returns_empty_vec() {
        let entries = decode_reloc_table(&[], 0, 0).unwrap();
        assert!(entries.is_empty());
    }
}
