//! Member-internal reloc patcher — S7-C5b.
//!
//! Pairs with `member_reloc::parse_member_text_relocs` (S7-C5a) —
//! takes a member's reloc entries plus the final-link sym table and
//! patches each affected instruction word / 8-byte data slot in
//! place against a mutable copy of the member's `__text` payload.
//!
//! The patcher mirrors `resolve::apply_relocs` for user functions
//! but operates on archive members: the symbol-name resolution path
//! has to walk the member's own `LC_SYMTAB` + strtab to map
//! `r_symbolnum → name` first, then look the resolved name up in
//! the link driver's sym table.

use torajs_obj::{
    ARM64_RELOC_BRANCH26, ARM64_RELOC_PAGE21, ARM64_RELOC_PAGEOFF12, ARM64_RELOC_UNSIGNED,
    LC_SYMTAB, NLIST_64_SIZE,
};

use crate::archive::ArMember;
use crate::member_reloc::{MemberRelocError, parse_member_text_relocs};
use crate::patch::{patch_branch26, patch_page21, patch_pageoff12, write_unsigned64};
use crate::resolve::SymTable;

/// Failures `apply_member_relocs` can surface in addition to the
/// `MemberRelocError` decoding stage. Variants split out the
/// post-decode failure modes so the caller can blame either "your
/// archive's wire format is broken" (decode) or "your link doesn't
/// supply this symbol" (apply).
#[derive(Debug, PartialEq, Eq)]
pub enum MemberRelocApplyError {
    /// Decoder stage already failed — bubble it up.
    Decode(MemberRelocError),
    /// Member's `LC_SYMTAB` symoff/nsyms or stroff/strsize would
    /// extend past the member bytes.
    TruncatedSymtab {
        symoff: usize,
        nsyms: usize,
        file_size: usize,
    },
    TruncatedStrtab {
        stroff: usize,
        strsize: usize,
        file_size: usize,
    },
    /// `r_symbolnum` is past the end of the member's nlist table.
    SymbolnumOutOfRange { r_symbolnum: u32, nsyms: u32 },
    /// `nlist[r_symbolnum].n_strx` indexes outside the strtab.
    BadStrx { sym_index: u32, n_strx: usize },
    /// Symbol name in strtab is not NUL-terminated.
    UnterminatedSymName { sym_index: u32, n_strx: usize },
    /// Symbol name bytes aren't valid UTF-8.
    NameNotUtf8 { sym_index: u32 },
    /// Member referenced a symbol no archive / sym_table defines —
    /// link is incomplete.
    UnresolvedSymbol { name: String },
    /// `r_extern = 0` (section-keyed reloc). Real `libtorajs_*.a`
    /// members emitted by rustc / cc may use these for local jumps;
    /// S7-C5 ships extern-only support, section-keyed lands later
    /// when production smoke surfaces it.
    SectionBasedRelocUnsupported { r_address: u32 },
    /// Reloc type isn't one of the four ARM64 kinds the patcher
    /// understands.
    UnknownRelocType { r_type: u8, r_address: u32 },
    /// Patch site or 8-byte data slot would extend past the
    /// member's __text payload.
    PatchSiteOutOfRange {
        r_address: u32,
        length_bytes: u32,
        text_size: usize,
    },
    /// BRANCH26 displacement doesn't fit in a 26-bit signed range
    /// (= 33-bit absolute range). Silent corruption would be worse
    /// than this loud failure.
    BranchDisplacementOverflow {
        site_vaddr: u64,
        target_vaddr: u64,
        displacement: i64,
    },
    /// `LC_SYMTAB` was missing entirely. Members emitted by
    /// `torajs_obj::write_object` always have one; a missing
    /// load command means the member isn't a torajs-obj output.
    NoSymtab,
}

impl From<MemberRelocError> for MemberRelocApplyError {
    fn from(e: MemberRelocError) -> Self {
        MemberRelocApplyError::Decode(e)
    }
}

/// Patch every member-internal reloc in `bytes` against the
/// final-link `sym_table`. `bytes` must be a fresh copy of the
/// member's `__TEXT,__text` payload (typically
/// `member.data[text_off..text_off + text_size].to_vec()`);
/// `member_vaddr` is the final virtual address that copy will
/// occupy in the linked image. After this call, `bytes` is the
/// patched __text slice ready to write into the binary's __TEXT.
///
/// `sym_table` must already include every defined-extern symbol
/// the member's relocs can reach — the caller (link driver) seeds
/// it from `cfg.sym_table` ∪ every integrated member's exported
/// `(name → vaddr)` pairs.
pub fn apply_member_relocs(
    bytes: &mut [u8],
    member: &ArMember<'_>,
    member_vaddr: u64,
    sym_table: &SymTable,
) -> Result<(), MemberRelocApplyError> {
    let relocs = parse_member_text_relocs(member)?;
    if relocs.is_empty() {
        return Ok(());
    }
    let symtab = parse_member_symtab(member)?;

    let text_size = bytes.len();
    for r in &relocs {
        if r.r_extern == 0 {
            return Err(MemberRelocApplyError::SectionBasedRelocUnsupported {
                r_address: r.r_address,
            });
        }
        if r.r_symbolnum >= symtab.nsyms {
            return Err(MemberRelocApplyError::SymbolnumOutOfRange {
                r_symbolnum: r.r_symbolnum,
                nsyms: symtab.nsyms,
            });
        }
        let name = read_nlist_name(member, &symtab, r.r_symbolnum)?;
        let target_vaddr = *sym_table
            .get(&name)
            .ok_or(MemberRelocApplyError::UnresolvedSymbol { name: name.clone() })?;

        let off = r.r_address as usize;
        let length_bytes = if r.r_type == ARM64_RELOC_UNSIGNED {
            8
        } else {
            4
        };
        if off
            .checked_add(length_bytes as usize)
            .map(|end| end > text_size)
            .unwrap_or(true)
        {
            return Err(MemberRelocApplyError::PatchSiteOutOfRange {
                r_address: r.r_address,
                length_bytes,
                text_size,
            });
        }

        let site_vaddr = member_vaddr + u64::from(r.r_address);
        match r.r_type {
            ARM64_RELOC_BRANCH26 => {
                let displacement = (target_vaddr as i64) - (site_vaddr as i64);
                let displacement_i32 = i32::try_from(displacement).map_err(|_| {
                    MemberRelocApplyError::BranchDisplacementOverflow {
                        site_vaddr,
                        target_vaddr,
                        displacement,
                    }
                })?;
                let insn = read_u32_le(bytes, off);
                let patched = patch_branch26(insn, displacement_i32);
                write_u32_le(bytes, off, patched);
            }
            ARM64_RELOC_PAGE21 => {
                let target_page = (target_vaddr & !0xFFF) as i64;
                let site_page = (site_vaddr & !0xFFF) as i64;
                let displacement = target_page - site_page;
                let insn = read_u32_le(bytes, off);
                let patched = patch_page21(insn, displacement);
                write_u32_le(bytes, off, patched);
            }
            ARM64_RELOC_PAGEOFF12 => {
                let offset12 = (target_vaddr & 0xFFF) as u32;
                let insn = read_u32_le(bytes, off);
                let patched = patch_pageoff12(insn, offset12);
                write_u32_le(bytes, off, patched);
            }
            ARM64_RELOC_UNSIGNED => {
                let slot: &mut [u8; 8] = (&mut bytes[off..off + 8])
                    .try_into()
                    .expect("PatchSiteOutOfRange guard already enforced 8-byte slot");
                write_unsigned64(slot, target_vaddr);
            }
            other => {
                return Err(MemberRelocApplyError::UnknownRelocType {
                    r_type: other,
                    r_address: r.r_address,
                });
            }
        }
    }
    Ok(())
}

/// Sub-view of a parsed `LC_SYMTAB`, the four absolute file offsets
/// + counts the symbol-name resolver needs.
#[derive(Debug, Clone, Copy)]
struct MemberSymtab {
    symoff: u32,
    nsyms: u32,
    stroff: u32,
    strsize: u32,
}

/// Walk a member's load commands and pull the `LC_SYMTAB` four-tuple.
/// Bounds-checks the symoff/stroff ranges against the member buffer
/// up front so the per-reloc loop in `apply_member_relocs` can index
/// without further validation.
fn parse_member_symtab(member: &ArMember<'_>) -> Result<MemberSymtab, MemberRelocApplyError> {
    let bytes = member.data;
    if bytes.len() < 32 {
        return Err(MemberRelocApplyError::Decode(
            MemberRelocError::TruncatedHeader,
        ));
    }
    let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let mut cursor = 32usize;
    let mut symtab: Option<MemberSymtab> = None;
    for _ in 0..ncmds {
        if bytes.len() < cursor + 8 {
            return Err(MemberRelocApplyError::Decode(
                MemberRelocError::TruncatedLoadCommand { offset: cursor },
            ));
        }
        let cmd = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        let cmdsize =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        if cmdsize < 8 || bytes.len() < cursor + cmdsize {
            return Err(MemberRelocApplyError::Decode(
                MemberRelocError::TruncatedLoadCommand { offset: cursor },
            ));
        }
        if cmd == LC_SYMTAB {
            if cmdsize < 24 {
                return Err(MemberRelocApplyError::Decode(
                    MemberRelocError::TruncatedLoadCommand { offset: cursor },
                ));
            }
            symtab = Some(MemberSymtab {
                symoff: u32::from_le_bytes(bytes[cursor + 8..cursor + 12].try_into().unwrap()),
                nsyms: u32::from_le_bytes(bytes[cursor + 12..cursor + 16].try_into().unwrap()),
                stroff: u32::from_le_bytes(bytes[cursor + 16..cursor + 20].try_into().unwrap()),
                strsize: u32::from_le_bytes(bytes[cursor + 20..cursor + 24].try_into().unwrap()),
            });
        }
        cursor += cmdsize;
    }
    let s = symtab.ok_or(MemberRelocApplyError::NoSymtab)?;

    let symoff_us = s.symoff as usize;
    let nsyms_us = s.nsyms as usize;
    let stroff_us = s.stroff as usize;
    let strsize_us = s.strsize as usize;
    let nlist_end = symoff_us
        .checked_add(nsyms_us * NLIST_64_SIZE as usize)
        .ok_or(MemberRelocApplyError::TruncatedSymtab {
            symoff: symoff_us,
            nsyms: nsyms_us,
            file_size: bytes.len(),
        })?;
    if nlist_end > bytes.len() {
        return Err(MemberRelocApplyError::TruncatedSymtab {
            symoff: symoff_us,
            nsyms: nsyms_us,
            file_size: bytes.len(),
        });
    }
    let strtab_end =
        stroff_us
            .checked_add(strsize_us)
            .ok_or(MemberRelocApplyError::TruncatedStrtab {
                stroff: stroff_us,
                strsize: strsize_us,
                file_size: bytes.len(),
            })?;
    if strtab_end > bytes.len() {
        return Err(MemberRelocApplyError::TruncatedStrtab {
            stroff: stroff_us,
            strsize: strsize_us,
            file_size: bytes.len(),
        });
    }
    Ok(s)
}

/// Resolve `nlist[sym_index].n_strx` to a UTF-8 symbol name.
fn read_nlist_name(
    member: &ArMember<'_>,
    symtab: &MemberSymtab,
    sym_index: u32,
) -> Result<String, MemberRelocApplyError> {
    let bytes = member.data;
    let nlist_off = symtab.symoff as usize + sym_index as usize * NLIST_64_SIZE as usize;
    let n_strx = u32::from_le_bytes(bytes[nlist_off..nlist_off + 4].try_into().unwrap()) as usize;
    let stroff = symtab.stroff as usize;
    let strsize = symtab.strsize as usize;
    if n_strx >= strsize {
        return Err(MemberRelocApplyError::BadStrx { sym_index, n_strx });
    }
    let strtab = &bytes[stroff..stroff + strsize];
    let end = strtab[n_strx..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| n_strx + p)
        .ok_or(MemberRelocApplyError::UnterminatedSymName { sym_index, n_strx })?;
    let name_bytes = &strtab[n_strx..end];
    std::str::from_utf8(name_bytes)
        .map(|s| s.to_string())
        .map_err(|_| MemberRelocApplyError::NameNotUtf8 { sym_index })
}

fn read_u32_le(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
}

fn write_u32_le(bytes: &mut [u8], off: usize, value: u32) {
    bytes[off..off + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;
    use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};
    use torajs_obj::write_object;

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

    fn fn_leaf(name: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: RET_BYTES.to_vec(),
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// `apply_member_relocs` on a leaf member is a no-op — bytes
    /// unchanged.
    #[test]
    fn leaf_member_is_noop() {
        let foo = fn_leaf("_foo");
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = crate::archive::parse_archive(&archive).unwrap();

        let (text_off, text_size) = crate::archive_link::parse_member_text_section(&members[0])
            .expect("parse text section");
        let off = text_off as usize;
        let end = off + text_size as usize;
        let mut bytes = members[0].data[off..end].to_vec();
        let pre = bytes.clone();

        let sym_table = SymTable::new();
        apply_member_relocs(&mut bytes, &members[0], 0x0000_0001_0000_4000, &sym_table)
            .expect("noop on leaf");
        assert_eq!(bytes, pre, "leaf member bytes must stay untouched");
    }

    /// Member `_foo` BL's extern `_bar`. Place `_foo` at vaddr
    /// 0x100004000 and resolve `_bar` to 0x100005000 via sym_table.
    /// After apply, the BL site at member __text byte 0 must encode
    /// displacement = +0x1000 = 4096 B → imm26 = 4096 / 4 = 1024.
    #[test]
    fn patches_extern_branch26() {
        let foo = foo_calls_bar();
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = crate::archive::parse_archive(&archive).unwrap();

        let (text_off, text_size) = crate::archive_link::parse_member_text_section(&members[0])
            .expect("parse text section");
        let off = text_off as usize;
        let end = off + text_size as usize;
        let mut bytes = members[0].data[off..end].to_vec();

        // Pre-patch: BL placeholder = 0x94000000.
        assert_eq!(
            u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            0x9400_0000,
        );

        let member_vaddr = 0x0000_0001_0000_4000u64;
        let mut sym_table = SymTable::new();
        sym_table.insert("_bar".into(), 0x0000_0001_0000_5000u64);

        apply_member_relocs(&mut bytes, &members[0], member_vaddr, &sym_table)
            .expect("apply_member_relocs");

        // Site = member_vaddr + 0 = 0x100004000; target = 0x100005000.
        // Displacement = +0x1000 = 4096 B → imm26 = 1024 = 0x400.
        let post = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        assert_eq!(post & 0x03FF_FFFF, 0x400);
        assert_eq!(post, 0x9400_0400);

        // RET at byte 4 untouched.
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            0xD65F_03C0,
        );
    }

    /// Unresolved symbol surfaces as a typed error, not a panic.
    /// Link drivers can surface "missing -lfoo" diagnostics from it.
    #[test]
    fn unresolved_symbol_returns_typed_error() {
        let foo = foo_calls_bar();
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = crate::archive::parse_archive(&archive).unwrap();

        let (text_off, text_size) = crate::archive_link::parse_member_text_section(&members[0])
            .expect("parse text section");
        let off = text_off as usize;
        let end = off + text_size as usize;
        let mut bytes = members[0].data[off..end].to_vec();

        let sym_table = SymTable::new();
        let err = apply_member_relocs(&mut bytes, &members[0], 0x100004000, &sym_table)
            .expect_err("missing _bar must fail");
        match err {
            MemberRelocApplyError::UnresolvedSymbol { name } => assert_eq!(name, "_bar"),
            other => panic!("expected UnresolvedSymbol, got {other:?}"),
        }
    }
}
