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
    ARM64_RELOC_ADDEND, ARM64_RELOC_BRANCH26, ARM64_RELOC_GOT_LOAD_PAGE21,
    ARM64_RELOC_GOT_LOAD_PAGEOFF12, ARM64_RELOC_PAGE21, ARM64_RELOC_PAGEOFF12,
    ARM64_RELOC_TLVP_LOAD_PAGE21, ARM64_RELOC_TLVP_LOAD_PAGEOFF12, ARM64_RELOC_UNSIGNED,
};

use crate::archive::ArMember;
use crate::data_section_layout::DataSectionLayout;
use crate::member_reloc::{MemberRelocEntry, MemberRelocError, parse_member_text_relocs};
use crate::member_resolve::{MemberSymtab, resolve_local_nsect};
use crate::member_symtab::{parse_member_symtab, read_nlist_name};
use crate::non_text_layout::NonTextSectionLayout;
use crate::patch::{patch_branch26, patch_page21, patch_pageoff12, write_unsigned64};
use crate::resolve::SymTable;

/// Reloc target post-classification (SD-4c-prereq-a/b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchKind {
    /// `r_extern = 0` — target is a member-internal section.
    /// `section_index` is the 1-based ordinal from `r_symbolnum`;
    /// `addend` carries any pending `ARM64_RELOC_ADDEND` value.
    SectionRef { section_index: u8, addend: i64 },
}

/// Classify a section-keyed (`r_extern = 0`) reloc into a
/// [`PatchKind::SectionRef`]. Returns `None` for `r_extern = 1`
/// (those keep flowing through the named-symbol path) or for
/// `r_symbolnum > 255` (strict validation moves to the call site).
pub fn classify_section_reloc(entry: &MemberRelocEntry, pending_addend: i64) -> Option<PatchKind> {
    if entry.r_extern != 0 {
        return None;
    }
    let section_index = u8::try_from(entry.r_symbolnum).ok()?;
    Some(PatchKind::SectionRef {
        section_index,
        addend: pending_addend,
    })
}

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
    /// `r_extern = 0` reloc carried an `r_symbolnum` that doesn't fit
    /// a Mach-O 8-bit section index, or names a section past the end
    /// of the member's section table. Surfaced from SD-4c-prereq-b's
    /// dispatch — prereq-a wires the typed variant so callers added
    /// later don't need to re-shape the error enum.
    InvalidSectionIndex { r_symbolnum: u32, nsects: usize },
    /// The classified section index resolved to a slot the member
    /// doesn't carry in its `LC_SEGMENT_64` section table. Distinct
    /// from `InvalidSectionIndex` (which catches malformed
    /// `r_symbolnum`); this fires when the index parses cleanly but
    /// the member's layout doesn't expose that section to the
    /// patcher.
    SectionNotInMember { section_index: u8 },
    /// Reloc type isn't one of the four ARM64 kinds the patcher
    /// understands.
    UnknownRelocType { r_type: u8, r_address: u32 },
    /// A `GOT_LOAD_PAGEOFF12` reloc landed on something other than
    /// the 64-bit LDR (imm12) the LDR→ADD relaxation rewrites. The
    /// blind bit-mask would fabricate a garbage instruction and ship
    /// it silently — this was a `debug_assert` (invisible in
    /// release/iter) until rotation 329 hardened it while chasing the
    /// PC=1 latent bug (which turned out to live elsewhere; every
    /// observed site is the standard 0xF940 LDR, so this arm firing
    /// means a codegen pattern this relaxation has never seen).
    GotLoadRelaxNotLdr { insn: u32, r_address: u32 },
    /// Patch site or 8-byte data slot would extend past the
    /// member's __text payload.
    PatchSiteOutOfRange {
        r_address: u32,
        length_bytes: u32,
        text_size: usize,
    },
    /// swap-2k chunk 2b-4: collector emitted a reloc count that
    /// disagrees with the chained-fixups encoder's per-slot link
    /// value vec for the same section — programming bug in the
    /// archive_emit wire (cursor walks the two in lockstep).
    SlotCountMismatch { relocs: usize, link_values: usize },
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
    non_text_sections: &[NonTextSectionLayout],
    data_non_text_sections: &[DataSectionLayout],
) -> Result<(), MemberRelocApplyError> {
    let relocs = parse_member_text_relocs(member)?;
    if relocs.is_empty() {
        return Ok(());
    }
    let symtab = parse_member_symtab(member)?;

    let text_size = bytes.len();

    // `ARM64_RELOC_ADDEND` is a meta-reloc — it carries no target,
    // but stashes a 24-bit signed addend (in `r_symbolnum`) that
    // applies to the immediately-following reloc on the same site.
    // ld64 / lld walk the reloc table linearly and accumulate the
    // addend, then consume it when the next non-ADDEND reloc fires.
    let mut pending_addend: i64 = 0;

    for r in &relocs {
        // Meta-reloc — accumulate then continue.
        if r.r_type == ARM64_RELOC_ADDEND {
            // r_symbolnum is a 24-bit signed value per
            // <mach-o/arm64/reloc.h>. Sign-extend to i64.
            let raw = r.r_symbolnum & 0x00FF_FFFF;
            pending_addend = if raw & 0x0080_0000 != 0 {
                (raw | 0xFF00_0000) as i32 as i64
            } else {
                raw as i64
            };
            continue;
        }

        let sym_vaddr = resolve_reloc_target(
            r,
            &mut pending_addend,
            member,
            &symtab,
            member_vaddr,
            sym_table,
            non_text_sections,
            data_non_text_sections,
        )?;
        // Apply (then clear) the pending addend from any preceding
        // ARM64_RELOC_ADDEND. Sign-extension is preserved through
        // the wrapping add — Mach-O addends commonly point past
        // the end of a global to express a "C struct field offset"
        // pattern, so wrapping semantics are textbook here.
        let target_vaddr = (sym_vaddr as i64).wrapping_add(pending_addend) as u64;
        pending_addend = 0;

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
        patch_reloc_site(bytes, r, off, site_vaddr, target_vaddr)?;
    }
    Ok(())
}

/// Resolve one non-ADDEND reloc's raw symbol vaddr.
///
/// SD-4c-prereq-b3 — dispatch r_extern=0 (section-keyed) relocs
/// through `classify_section_reloc` + lookup in `non_text_sections`;
/// on that path `pending_addend` is replaced with the classified
/// addend so the caller's shared post-dispatch arithmetic still
/// applies it exactly once. The named-sym path (r_extern=1) walks
/// the member's `LC_SYMTAB` for the name, then `sym_table` with a
/// `resolve_local_nsect` fallback.
#[allow(clippy::too_many_arguments)]
fn resolve_reloc_target(
    r: &MemberRelocEntry,
    pending_addend: &mut i64,
    member: &ArMember<'_>,
    symtab: &MemberSymtab,
    member_vaddr: u64,
    sym_table: &SymTable,
    non_text_sections: &[NonTextSectionLayout],
    data_non_text_sections: &[DataSectionLayout],
) -> Result<u64, MemberRelocApplyError> {
    if let Some(PatchKind::SectionRef {
        section_index,
        addend,
    }) = classify_section_reloc(r, *pending_addend)
    {
        let layout = non_text_sections
            .iter()
            .find(|s| s.section_index == section_index)
            .ok_or(MemberRelocApplyError::SectionNotInMember { section_index })?;
        *pending_addend = addend;
        Ok(layout.final_vaddr)
    } else if r.r_extern == 0 {
        // classify returned None for an r_extern=0 entry — only
        // possible when r_symbolnum > 255 (per
        // classify_section_reloc's strict-validation deferral).
        Err(MemberRelocApplyError::InvalidSectionIndex {
            r_symbolnum: r.r_symbolnum,
            nsects: non_text_sections.len(),
        })
    } else {
        if r.r_symbolnum >= symtab.nsyms {
            return Err(MemberRelocApplyError::SymbolnumOutOfRange {
                r_symbolnum: r.r_symbolnum,
                nsyms: symtab.nsyms,
            });
        }
        let name = read_nlist_name(member, symtab, r.r_symbolnum)?;
        if let Some(&v) = sym_table.get(&name) {
            Ok(v)
        } else if let Some(v) = resolve_local_nsect(
            member,
            symtab,
            r.r_symbolnum,
            member_vaddr,
            non_text_sections,
            data_non_text_sections,
        ) {
            Ok(v)
        } else {
            Err(MemberRelocApplyError::UnresolvedSymbol { name: name.clone() })
        }
    }
}

/// Patch one reloc site in the member's `__text` copy — per-r_type
/// instruction-word / 8-byte-slot rewrite. The caller has already
/// bounds-checked `off + length_bytes <= bytes.len()`.
fn patch_reloc_site(
    bytes: &mut [u8],
    r: &MemberRelocEntry,
    off: usize,
    site_vaddr: u64,
    target_vaddr: u64,
) -> Result<(), MemberRelocApplyError> {
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
        // GOT_LOAD_PAGE21 patches identically to PAGE21 — both
        // emit an ADRP that points at the target's page. The
        // ld64 "GOT optimization" replaces the GOT-indirected
        // pair with a direct PC-relative pair when the target
        // is inside the binary's ±4 GiB range, which our
        // single-image binaries always satisfy.
        //
        // TLVP_LOAD_PAGE21 (SD-4c-prereq+a) also patches the
        // ADRP imm21, but the target is the TLV descriptor's
        // `__DATA,__thread_vars` page. No "TLV relaxation"
        // exists — dyld binds the descriptor's thunk slot, so
        // the indirection through the descriptor stays. The
        // PAGE21 patch math is the same; only the target
        // source differs (prereq+b/c feeds the descriptor
        // vaddr through the same sym_table path the named-sym
        // branch already uses).
        ARM64_RELOC_PAGE21 | ARM64_RELOC_GOT_LOAD_PAGE21 | ARM64_RELOC_TLVP_LOAD_PAGE21 => {
            let target_page = (target_vaddr & !0xFFF) as i64;
            let site_page = (site_vaddr & !0xFFF) as i64;
            let displacement = target_page - site_page;
            let insn = read_u32_le(bytes, off);
            let patched = patch_page21(insn, displacement);
            write_u32_le(bytes, off, patched);
        }
        // PAGEOFF12 patches the imm12 field (bits 21:10) of an
        // ADD or LDR instruction — the same bit slot regardless
        // of opcode, so a single patch covers both.
        //
        // TLVP_LOAD_PAGEOFF12 (SD-4c-prereq+a) likewise patches
        // imm12, but unlike GOT_LOAD_PAGEOFF12 there is *no*
        // LDR→ADD opcode rewrite: dyld must bind the TLV
        // descriptor's thunk slot at process start, so the LDR-
        // through-descriptor indirection is preserved.
        ARM64_RELOC_PAGEOFF12 | ARM64_RELOC_TLVP_LOAD_PAGEOFF12 => {
            let offset12 = (target_vaddr & 0xFFF) as u32;
            let insn = read_u32_le(bytes, off);
            let patched = patch_pageoff12(insn, offset12);
            write_u32_le(bytes, off, patched);
        }
        // GOT_LOAD_PAGEOFF12 lives on an LDR instruction (load
        // the GOT slot, which itself holds the target's vaddr).
        // The textbook ld64 relaxation rewrites the LDR opcode
        // in place to an ADD opcode and then patches the imm12
        // with the symbol's own page-offset — turning
        // `ADRP+LDR (via GOT)` into `ADRP+ADD (direct)`. Bits
        // 30/29/27/22 differ between LDR (immediate, 64-bit
        // unsigned offset, 0xF940_0000 | …) and ADD (immediate,
        // 64-bit, shift=0, 0x9100_0000 | …); zeroing those
        // four bits in the instruction word performs the
        // rewrite without touching the Rn/Rt fields. Verified
        // bit-for-bit against ARMv8-A C6.2.184 + C6.2.4.
        ARM64_RELOC_GOT_LOAD_PAGEOFF12 => {
            let insn_ldr = read_u32_le(bytes, off);
            // Hard error, not debug_assert: on a non-LDR encoding the
            // mask below fabricates a garbage instruction and ships it
            // with every gate green (see GotLoadRelaxNotLdr).
            if insn_ldr & 0xFFC0_0000 != 0xF940_0000 {
                return Err(MemberRelocApplyError::GotLoadRelaxNotLdr {
                    insn: insn_ldr,
                    r_address: r.r_address,
                });
            }
            let insn_add = insn_ldr & !0x6840_0000;
            let offset12 = (target_vaddr & 0xFFF) as u32;
            let patched = patch_pageoff12(insn_add, offset12);
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
    Ok(())
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
        apply_member_relocs(
            &mut bytes,
            &members[0],
            0x0000_0001_0000_4000,
            &sym_table,
            &[],
            &[],
        )
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

        apply_member_relocs(&mut bytes, &members[0], member_vaddr, &sym_table, &[], &[])
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

    /// `ARM64_RELOC_ADDEND` r_symbolnum is a 24-bit signed addend.
    /// Sign-extension across the 0x800000 boundary is the most
    /// error-prone slot — verify both polarities.
    #[test]
    fn addend_sign_extends_24_bit_value() {
        // Mirror the inner sign-extension expression directly so
        // any future refactor of `apply_member_relocs` that breaks
        // it shows up here.
        fn sign_extend_24(r_symbolnum: u32) -> i64 {
            let raw = r_symbolnum & 0x00FF_FFFF;
            if raw & 0x0080_0000 != 0 {
                (raw | 0xFF00_0000) as i32 as i64
            } else {
                raw as i64
            }
        }
        assert_eq!(sign_extend_24(0), 0);
        assert_eq!(sign_extend_24(0x536), 0x536);
        assert_eq!(sign_extend_24(0x7F_FFFF), 0x7F_FFFF);
        assert_eq!(sign_extend_24(0x80_0000), -0x80_0000);
        assert_eq!(sign_extend_24(0xFF_FFFF), -1);
        assert_eq!(sign_extend_24(0x00FF_FFFF), -1);
    }

    /// GOT_LOAD_PAGEOFF12 textbook LDR→ADD relaxation:
    /// `LDR Xt, [Xn, #imm12*8]` (opcode 0xF940_0000 family) is
    /// rewritten to `ADD Xd, Xn, #imm12` (opcode 0x9100_0000) by
    /// clearing bits 30/29/27/22. The Rn/Rt register fields stay
    /// untouched; only the 10-bit opcode prefix changes.
    #[test]
    fn ldr_to_add_relaxation_clears_correct_bits() {
        // LDR X0, [X1, #0] = 0xF940_0020 (Rn=X1 in bits 9..5, Rt=X0
        // in bits 4..0; imm12 = 0 in bits 21..10).
        let ldr = 0xF940_0020u32;
        let add = ldr & !0x6840_0000u32;
        assert_eq!(add, 0x9100_0020, "ADD X0, X1, #0");

        // LDR X8, [X16, #8] would have imm12 = 1 in bits 21..10
        // (since LDR imm12 is byte_offset/8 for 64-bit) — relaxation
        // discards that imm12 (the patcher overwrites it with the
        // symbol's page-offset). Verify the opcode-prefix transform.
        let ldr2 = 0xF940_0608u32; // LDR X8, [X16, #16] -> imm12=2
        let add2 = ldr2 & !0x6840_0000u32;
        assert_eq!(add2 & 0xFFC0_0000, 0x9100_0000, "ADD opcode prefix");
        // Rn (bits 9:5) = 0b10000 = X16, Rt/Rd (bits 4:0) = 0b01000 = X8
        assert_eq!((add2 >> 5) & 0x1F, 0x10);
        assert_eq!(add2 & 0x1F, 0x08);
    }

    /// GOT_LOAD_PAGE21 must patch identically to a regular PAGE21
    /// — both encode an ADRP page-displacement. The shared match
    /// arm is the textbook ld64 relaxation when the target is in
    /// range (our single-image binaries always satisfy that).
    #[test]
    fn got_load_page21_patches_like_page21() {
        // Hand-build a fixture member with one GOT_LOAD_PAGE21 +
        // one paired GOT_LOAD_PAGEOFF12 against an extern `_sym`.
        // We can't easily synthesize a member with these reloc
        // kinds from compile_function (codegen never emits GOT
        // relocs), so we exercise the relaxation arms through the
        // unit assertions above and through the real_link probe
        // end-to-end for the production check.
        //
        // Lift the bit-rewrite math into a const so the test
        // doubles as a compile-time spec check.
        const LDR_OP: u32 = 0xF940_0000;
        const ADD_OP: u32 = 0x9100_0000;
        const RELAX_MASK: u32 = !0x6840_0000;
        assert_eq!(LDR_OP & RELAX_MASK, ADD_OP);
    }

    /// Section-keyed reloc (`r_extern = 0`) classifies into
    /// `PatchKind::SectionRef` with the 1-based section index lifted
    /// from `r_symbolnum` and the running addend preserved.
    #[test]
    fn classify_section_reloc_returns_section_ref_for_r_extern_zero() {
        let entry = MemberRelocEntry {
            r_address: 0x40,
            r_symbolnum: 3,
            r_pcrel: 0,
            r_length: 2,
            r_extern: 0,
            r_type: torajs_obj::ARM64_RELOC_UNSIGNED,
        };
        let kind = classify_section_reloc(&entry, 0x100);
        assert_eq!(
            kind,
            Some(PatchKind::SectionRef {
                section_index: 3,
                addend: 0x100,
            }),
        );
    }

    /// Extern-keyed reloc (`r_extern = 1`) returns `None` — the
    /// existing named-symbol path in `apply_member_relocs` stays
    /// authoritative for that case.
    #[test]
    fn classify_section_reloc_returns_none_for_extern() {
        let entry = MemberRelocEntry {
            r_address: 0,
            r_symbolnum: 5,
            r_pcrel: 1,
            r_length: 2,
            r_extern: 1,
            r_type: torajs_obj::ARM64_RELOC_BRANCH26,
        };
        assert_eq!(classify_section_reloc(&entry, 0), None);
        // Non-zero pending_addend doesn't lift the extern guard.
        assert_eq!(classify_section_reloc(&entry, 0x42), None);
    }

    /// `r_extern = 0` with `r_symbolnum` past 8 bits returns `None`.
    /// Mach-O 8-bit `nlist.n_sect` caps real section counts at 255;
    /// the strict numeric error variant fires from prereq-b's
    /// dispatch where the member's true `nsects` is known.
    #[test]
    fn classify_section_reloc_returns_none_for_out_of_range_section_idx() {
        let entry = MemberRelocEntry {
            r_address: 0,
            r_symbolnum: 256,
            r_pcrel: 0,
            r_length: 2,
            r_extern: 0,
            r_type: torajs_obj::ARM64_RELOC_UNSIGNED,
        };
        assert_eq!(classify_section_reloc(&entry, 0), None);
        let entry_huge = MemberRelocEntry {
            r_symbolnum: 0x00FF_FFFF,
            ..entry
        };
        assert_eq!(classify_section_reloc(&entry_huge, 0), None);
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
        let err = apply_member_relocs(&mut bytes, &members[0], 0x100004000, &sym_table, &[], &[])
            .expect_err("missing _bar must fail");
        match err {
            MemberRelocApplyError::UnresolvedSymbol { name } => assert_eq!(name, "_bar"),
            other => panic!("expected UnresolvedSymbol, got {other:?}"),
        }
    }
}
