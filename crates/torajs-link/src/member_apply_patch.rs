//! Per-site instruction / slot rewrites for member `__text` relocs,
//! split out of `member_apply` so the walk and the encodings stay in
//! separate files.

use torajs_obj::{
    ARM64_RELOC_BRANCH26, ARM64_RELOC_GOT_LOAD_PAGE21, ARM64_RELOC_GOT_LOAD_PAGEOFF12,
    ARM64_RELOC_PAGE21, ARM64_RELOC_PAGEOFF12, ARM64_RELOC_TLVP_LOAD_PAGE21,
    ARM64_RELOC_TLVP_LOAD_PAGEOFF12, ARM64_RELOC_UNSIGNED,
};

use crate::member_apply::MemberRelocApplyError;
use crate::member_reloc::MemberRelocEntry;
use crate::patch::{patch_branch26, patch_page21, patch_pageoff12, write_unsigned64};

/// Patch one reloc site in the member's `__text` copy — per-r_type
/// instruction-word / 8-byte-slot rewrite. The caller has already
/// bounds-checked `off + length_bytes <= bytes.len()`.
pub(crate) fn patch_reloc_site(
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
            let patched =
                patch_pageoff12(insn, offset12).map_err(|_| misaligned(insn, target_vaddr, r))?;
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
            let patched = patch_pageoff12(insn_add, offset12)
                .map_err(|_| misaligned(insn_add, target_vaddr, r))?;
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

fn misaligned(insn: u32, target_vaddr: u64, r: &MemberRelocEntry) -> MemberRelocApplyError {
    MemberRelocApplyError::PageOff12Misaligned {
        insn,
        target_vaddr,
        r_address: r.r_address,
    }
}

fn read_u32_le(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
}

fn write_u32_le(bytes: &mut [u8], off: usize, value: u32) {
    bytes[off..off + 4].copy_from_slice(&value.to_le_bytes());
}

/// Point a `GOT_LOAD_PAGE21` / `GOT_LOAD_PAGEOFF12` pair at the 8-byte
/// slot `slot_vaddr` and keep the `LDR`, so the code loads the target's
/// address from the slot at run time.
pub(crate) fn patch_got_slot_site(
    bytes: &mut [u8],
    r: &MemberRelocEntry,
    pending_addend: i64,
    name: &str,
    site_vaddr: u64,
    slot_vaddr: u64,
) -> Result<(), MemberRelocApplyError> {
    let off = r.r_address as usize;
    let insn = read_u32_le(bytes, off);
    let patched = match r.r_type {
        ARM64_RELOC_GOT_LOAD_PAGE21 if pending_addend == 0 => {
            let disp = (slot_vaddr & !0xFFF) as i64 - (site_vaddr & !0xFFF) as i64;
            patch_page21(insn, disp)
        }
        ARM64_RELOC_GOT_LOAD_PAGEOFF12 if pending_addend == 0 => {
            if insn & 0xFFC0_0000 != 0xF940_0000 {
                return Err(MemberRelocApplyError::GotLoadRelaxNotLdr {
                    insn,
                    r_address: r.r_address,
                });
            }
            patch_pageoff12(insn, (slot_vaddr & 0xFFF) as u32)
                .map_err(|_| misaligned(insn, slot_vaddr, r))?
        }
        _ => {
            return Err(MemberRelocApplyError::GotSlotNotGotLoad {
                name: name.to_string(),
                r_type: r.r_type,
                r_address: r.r_address,
            });
        }
    };
    write_u32_le(bytes, off, patched);
    Ok(())
}
