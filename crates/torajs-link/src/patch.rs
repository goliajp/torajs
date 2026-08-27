//! ARM64 relocation patchers — pure bit-twiddling that takes a
//! resolved displacement / offset / absolute address and returns
//! the matching instruction word (or writes into a data slot).
//!
//! Encoding references:
//!
//! - `BL` / `B` — ARMv8-A ARM C6.2.36 / C6.2.37. Bits 25..0 hold a
//!   signed 26-bit immediate; the branch target is
//!   `site + (sign_extend(imm26) << 2)`.
//! - `ADRP` — ARMv8-A ARM C6.2.10. The 21-bit immediate is split:
//!   bits 30..29 carry `immlo` (low 2 bits) and bits 23..5 carry
//!   `immhi` (high 19 bits). The target is
//!   `(site & ~0xFFF) + (sign_extend(imm21) << 12)`.
//! - `ADD (immediate)` — ARMv8-A ARM C6.2.4. Bits 21..10 carry the
//!   12-bit immediate.
//! - `ARM64_RELOC_UNSIGNED` — 8-byte little-endian absolute address.
//!
//! Each patcher takes the unpatched instruction word (with the
//! immediate bits **already cleared to 0** by the codegen — that's
//! how `torajs_codegen` emits BL/ADRP/ADD reloc sites) and returns
//! the patched word. Sub-step S2's link driver feeds the resolved
//! addresses through these helpers in place against
//! `CompiledFunction.bytes` before the final exec image is laid
//! out.

/// Bit-mask covering bits 25..0 of an instruction — the `imm26`
/// slot of `B` and `BL`.
const BRANCH26_MASK: u32 = 0x03FF_FFFF;

/// Bit-mask covering bits 30..29 ∪ 23..5 of an instruction — the
/// composite (`immlo`, `immhi`) slot of `ADRP`.
const PAGE21_MASK: u32 = 0x60FF_FFE0;

/// Bit-mask covering bits 21..10 of an instruction — the `imm12`
/// slot of `ADD (immediate)`.
const PAGEOFF12_MASK: u32 = 0x003F_FC00;

/// Patch a `BL` / `B` instruction word with a signed PC-relative
/// branch displacement `target_addr - site_addr` (in bytes).
///
/// The displacement must be 4-byte aligned (every aarch64
/// instruction is 4 bytes) and must fit in a 26-bit signed value
/// times 4 — i.e. `± 128 MiB`. Out-of-range / unaligned input
/// triggers a debug-assert panic; silently truncating would emit
/// a wrong branch.
pub fn patch_branch26(insn: u32, displacement_bytes: i32) -> u32 {
    debug_assert_eq!(
        displacement_bytes % 4,
        0,
        "BRANCH26 displacement {displacement_bytes} must be 4-byte aligned"
    );
    let imm26 = displacement_bytes / 4;
    debug_assert!(
        (-(1 << 25)..(1 << 25)).contains(&imm26),
        "BRANCH26 imm26 {imm26} (displacement {displacement_bytes} B) overflows 26-bit signed range",
    );
    let masked_imm26 = (imm26 as u32) & BRANCH26_MASK;
    (insn & !BRANCH26_MASK) | masked_imm26
}

/// Patch an `ADRP` instruction word with a signed PC-relative
/// 4 KiB-page displacement (`(target & ~0xFFF) - (site & ~0xFFF)`,
/// in bytes — the patcher shifts internally).
///
/// `imm21 = page_displacement_bytes >> 12`, signed, fitting in
/// 21 bits — i.e. the target can reach `± 4 GiB` from the ADRP
/// site. Page-misalignment or out-of-range input triggers a
/// debug-assert panic.
pub fn patch_page21(insn: u32, page_displacement_bytes: i64) -> u32 {
    debug_assert_eq!(
        page_displacement_bytes & 0xFFF,
        0,
        "PAGE21 displacement {page_displacement_bytes} must be 4 KiB-aligned"
    );
    let imm21 = page_displacement_bytes >> 12;
    debug_assert!(
        (-(1 << 20)..(1 << 20)).contains(&imm21),
        "PAGE21 imm21 {imm21} (page displacement {page_displacement_bytes} B) overflows 21-bit signed range",
    );
    let imm21_u = imm21 as u32;
    let immlo = (imm21_u & 0b11) << 29;
    let immhi = ((imm21_u >> 2) & 0x7_FFFF) << 5;
    (insn & !PAGE21_MASK) | immlo | immhi
}

/// Patch the 12-bit `imm12` slot (bits 21:10) of an `ADD (immediate)`
/// or scaled load/store (LDR/STR, unsigned immediate) instruction with
/// the target's page-offset (`target_addr & 0xFFF`).
///
/// ADD and load/store share the same `imm12` bit slot but encode the
/// offset differently. ADD takes the raw byte offset. A load/store
/// stores the byte offset *divided by its access size* (the CPU
/// re-multiplies by the element width at execution). Writing the raw
/// byte offset into a scaled load therefore inflates the effective
/// displacement by the access size — e.g. a `ldr x` (8-byte) at +0x1F0
/// would dereference +0xF80. ld64 inspects the opcode and rescales;
/// this mirrors it (see [`load_store_scale`]).
///
/// Offsets ≥ 4096 (= 0x1000), or a load/store offset not aligned to
/// its access size, trigger a debug-assert panic — both would silently
/// corrupt the encoding. Apple ld emits the matching ADRP+ADD/LDR pair
/// so the offset is always within a single page and naturally aligned
/// by construction.
pub fn patch_pageoff12(insn: u32, offset: u32) -> u32 {
    debug_assert!(
        offset < 0x1000,
        "PAGEOFF12 offset 0x{offset:X} must fit in 12 bits"
    );
    let scale = load_store_scale(insn);
    debug_assert_eq!(
        offset & ((1 << scale) - 1),
        0,
        "PAGEOFF12 offset 0x{offset:X} not aligned to load/store access size 1<<{scale}"
    );
    let imm12 = ((offset >> scale) & 0xFFF) << 10;
    (insn & !PAGEOFF12_MASK) | imm12
}

/// Access-size log2 (`scale`) of a load/store *unsigned immediate*
/// instruction, or `0` for any other opcode (ADD/SUB take the raw
/// offset).
///
/// The load/store unsigned-immediate family is identified by
/// `(insn & 0x3B00_0000) == 0x3900_0000`. The scale is the 2-bit
/// `size` field (bits 31:30) for integer and ≤64-bit SIMD&FP accesses;
/// 128-bit SIMD&FP (`V` bit 26 and `opc`-high bit 23 both set) is a
/// 16-byte access with scale 4. Matches cctools/ld64's ARM64
/// PAGEOFF12 rescaling.
fn load_store_scale(insn: u32) -> u32 {
    const LOAD_STORE_UIMM_MASK: u32 = 0x3B00_0000;
    const LOAD_STORE_UIMM_VAL: u32 = 0x3900_0000;
    if insn & LOAD_STORE_UIMM_MASK != LOAD_STORE_UIMM_VAL {
        return 0;
    }
    if insn & 0x0480_0000 == 0x0480_0000 {
        return 4;
    }
    insn >> 30
}

/// Write an 8-byte little-endian absolute address into a data
/// slot — the `ARM64_RELOC_UNSIGNED` resolution path. Used for
/// static function pointers and any place the
/// codegen emitted a zero-filled 8-byte hole expecting a fixup.
pub fn write_unsigned64(slot: &mut [u8; 8], target_addr: u64) {
    slot.copy_from_slice(&target_addr.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- patch_branch26 ----

    /// `BL #target` from site 0 to target 16: displacement = +16 B,
    /// imm26 = 4. The base `BL` opcode word with imm26 = 0 is
    /// 0x94000000.
    #[test]
    fn branch26_forward_4_inst() {
        let bl_base = 0x9400_0000;
        let patched = patch_branch26(bl_base, 16);
        assert_eq!(patched, 0x9400_0004);
        // Sanity: bottom 26 bits = 4.
        assert_eq!(patched & 0x03FF_FFFF, 4);
    }

    /// Backwards branch: displacement = -16 B, imm26 = -4 =
    /// 0x03FFFFFC in two's complement (26 bits).
    #[test]
    fn branch26_backward_4_inst() {
        let bl_base = 0x9400_0000;
        let patched = patch_branch26(bl_base, -16);
        // imm26 = -4 in 26-bit two's complement = 0x03FFFFFC.
        assert_eq!(patched & 0x03FF_FFFF, 0x03FF_FFFC);
        // Top 6 bits stay = 0x94 >> 0 zeroed in low bits, top
        // unaffected.
        assert_eq!(patched >> 26, 0x94 >> 2);
    }

    #[test]
    fn branch26_zero_displacement_round_trip() {
        let bl_base = 0x9400_0000;
        assert_eq!(patch_branch26(bl_base, 0), bl_base);
    }

    /// Maximum positive displacement = (2^25 - 1) * 4 = 134217724 B
    /// → imm26 = 2^25 - 1 = 0x01FFFFFF.
    #[test]
    fn branch26_max_positive_displacement() {
        let bl_base = 0x9400_0000;
        let max_disp: i32 = ((1 << 25) - 1) * 4;
        let patched = patch_branch26(bl_base, max_disp);
        assert_eq!(patched & 0x03FF_FFFF, 0x01FF_FFFF);
    }

    /// Maximum negative displacement = -(2^25) * 4 = -134217728 B
    /// → imm26 = -2^25 = 0x02000000 (26-bit two's complement).
    #[test]
    fn branch26_max_negative_displacement() {
        let bl_base = 0x9400_0000;
        let min_disp: i32 = -(1 << 25) * 4;
        let patched = patch_branch26(bl_base, min_disp);
        assert_eq!(patched & 0x03FF_FFFF, 0x0200_0000);
    }

    /// Patching preserves the opcode (top 6 bits) of `B`
    /// (0x14000000) vs `BL` (0x94000000) — only the imm26 slot
    /// changes.
    #[test]
    fn branch26_preserves_opcode_bits() {
        let b_base = 0x1400_0000;
        let bl_base = 0x9400_0000;
        assert_eq!(patch_branch26(b_base, 16) & !0x03FF_FFFF, b_base);
        assert_eq!(patch_branch26(bl_base, 16) & !0x03FF_FFFF, bl_base);
    }

    // `debug_assert!` panic-guard tests — only fire in debug builds
    // (release optimizes them out, matching torajs-codegen's
    // `patch_branch` convention). Gated so `cargo test --release`
    // doesn't flag them as "did not panic as expected".
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "BRANCH26 displacement")]
    fn branch26_unaligned_panics() {
        let _ = patch_branch26(0x9400_0000, 5);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "BRANCH26 imm26")]
    fn branch26_overflow_positive_panics() {
        let _ = patch_branch26(0x9400_0000, (1 << 25) * 4); // imm26 = 2^25, out of range
    }

    // ---- patch_page21 ----

    /// ADRP target page = site page → imm21 = 0; word unchanged.
    /// Base `ADRP X0, #0` opcode = 0x90000000.
    #[test]
    fn page21_zero_displacement_round_trip() {
        let adrp_base = 0x9000_0000;
        assert_eq!(patch_page21(adrp_base, 0), adrp_base);
    }

    /// ADRP one page forward (`+0x1000`): imm21 = 1.
    /// imm21 = 0b00000_00000_00000_00001 — immlo = 0b01 (bit
    /// 30..29), immhi = 0 (bit 23..5).
    /// patched = base | (1 << 29) = 0x90000000 | 0x20000000 =
    ///           0x90000000 ∪ 0x20000000 → 0x60000000 cleared
    ///           portion + 0x20000000 = 0xB0000000.
    /// Sanity: opcode top-3 bits (bit 31..29) = `1 (op) | 0
    /// (immlo[1]) | 1 (immlo[0])` = 0b101.
    #[test]
    fn page21_one_page_forward() {
        let adrp_base = 0x9000_0000;
        let patched = patch_page21(adrp_base, 0x1000);
        // immlo = 1 << 29 = 0x20000000
        // immhi = 0
        let expected = (adrp_base & !PAGE21_MASK) | 0x2000_0000;
        assert_eq!(patched, expected);
    }

    /// ADRP backward one page (`-0x1000`): imm21 = -1 → 21-bit
    /// two's complement = 0x1FFFFF. immlo = 0b11 (bit 30..29) =
    /// 0x60000000. immhi = 0b111_1111_1111_1111_1111 (19 ones)
    /// at bit 23..5 = 0x00FFFFE0.
    #[test]
    fn page21_one_page_backward() {
        let adrp_base = 0x9000_0000;
        let patched = patch_page21(adrp_base, -0x1000);
        // Expected immlo = 0b11 << 29 = 0x60000000.
        // Expected immhi = 0x7FFFF << 5 = 0x00FFFFE0.
        let expected = (adrp_base & !PAGE21_MASK) | 0x6000_0000 | 0x00FF_FFE0;
        assert_eq!(patched, expected);
        // The full 21 immediate bits unpack back to -1.
        let immlo = (patched >> 29) & 0b11;
        let immhi = (patched >> 5) & 0x7_FFFF;
        let imm21 = (immhi << 2) | immlo;
        let sign_extended = ((imm21 as i32) << 11) >> 11;
        assert_eq!(sign_extended, -1);
    }

    /// ADRP forward to page 0x42000 from site page 0: imm21 =
    /// 0x42 = 66 = 0b00000_00000_00000_1000010.
    ///   immlo (bits 1..0 of imm21) = 0b10 = 2 → patched bit
    ///                                30..29 = 0b10.
    ///   immhi (bits 20..2 of imm21) = 0x42 >> 2 = 0x10.
    ///                              At bit 23..5 = 0x10 << 5 =
    ///                              0x200.
    #[test]
    fn page21_arbitrary_page_displacement() {
        let adrp_base = 0x9000_0000;
        let patched = patch_page21(adrp_base, 0x42000);
        let expected = (adrp_base & !PAGE21_MASK) | (0b10 << 29) | (0x10 << 5);
        assert_eq!(patched, expected);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "PAGE21 displacement")]
    fn page21_unaligned_panics() {
        let _ = patch_page21(0x9000_0000, 0x800);
    }

    // ---- patch_pageoff12 ----

    /// ADD imm12 = 0: word unchanged.
    /// Base `ADD X0, X0, #0` opcode = 0x91000000.
    #[test]
    fn pageoff12_zero_round_trip() {
        let add_base = 0x9100_0000;
        assert_eq!(patch_pageoff12(add_base, 0), add_base);
    }

    /// ADD imm12 = 0x123 lands at bit 21..10 = 0x123 << 10 =
    /// 0x48C00.
    #[test]
    fn pageoff12_arbitrary_offset() {
        let add_base = 0x9100_0000;
        let patched = patch_pageoff12(add_base, 0x123);
        let expected = (add_base & !PAGEOFF12_MASK) | (0x123 << 10);
        assert_eq!(patched, expected);
        // Round-trip extract:
        assert_eq!(((patched >> 10) & 0xFFF), 0x123);
    }

    /// ADD imm12 = 0xFFF (max 12-bit value).
    #[test]
    fn pageoff12_max_offset() {
        let add_base = 0x9100_0000;
        let patched = patch_pageoff12(add_base, 0xFFF);
        assert_eq!((patched >> 10) & 0xFFF, 0xFFF);
        // Bit 22 (the `sh` shift bit) stays 0 — the patcher
        // touches imm12 only.
        assert_eq!((patched >> 22) & 1, 0);
    }

    /// Patching does not flip the `sh` shift bit (bit 22). The
    /// codegen pre-zeroes the imm12 slot but the ADD opcode shape
    /// must round-trip exactly.
    #[test]
    fn pageoff12_preserves_sh_bit() {
        // Force the sh bit on in the input — patcher must not
        // clear it.
        let add_with_sh = 0x9100_0000 | (1 << 22);
        let patched = patch_pageoff12(add_with_sh, 0x100);
        assert_eq!((patched >> 22) & 1, 1, "sh bit must survive patching");
        assert_eq!((patched >> 10) & 0xFFF, 0x100);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "PAGEOFF12 offset")]
    fn pageoff12_overflow_panics() {
        let _ = patch_pageoff12(0x9100_0000, 0x1000);
    }

    /// A 64-bit `LDR Xt, [Xn, #imm]` (scale 3) must store the byte
    /// offset divided by 8 in imm12. Regression for the
    /// `__torajs_main_exit_code` Mutex::unlock crash: offset 0x1F0 was
    /// being written raw, so the CPU re-scaled it to +0xF80 and
    /// dereferenced a NULL slot. `LDR X0, [X8]` base = 0xF9400000.
    #[test]
    fn pageoff12_scales_ldr64() {
        let ldr_x = 0xF940_0000;
        let patched = patch_pageoff12(ldr_x, 0x1F0);
        // imm12 = 0x1F0 >> 3 = 0x3E.
        assert_eq!((patched >> 10) & 0xFFF, 0x3E);
        // Effective offset the CPU computes = imm12 << 3 = 0x1F0.
        assert_eq!(((patched >> 10) & 0xFFF) << 3, 0x1F0);
    }

    /// A 32-bit `LDR Wt, [Xn, #imm]` (scale 2) stores offset / 4.
    /// `LDR W0, [X0]` base = 0xB9400000.
    #[test]
    fn pageoff12_scales_ldr32() {
        let ldr_w = 0xB940_0000;
        let patched = patch_pageoff12(ldr_w, 0x40);
        assert_eq!((patched >> 10) & 0xFFF, 0x10);
    }

    /// A byte `LDRB Wt, [Xn, #imm]` (scale 0) stores the raw offset,
    /// same as ADD. `LDRB W0, [X0]` base = 0x39400000.
    #[test]
    fn pageoff12_ldrb_unscaled() {
        let ldrb = 0x3940_0000;
        let patched = patch_pageoff12(ldrb, 0x1F0);
        assert_eq!((patched >> 10) & 0xFFF, 0x1F0);
    }

    /// A 128-bit SIMD&FP `LDR Qt, [Xn, #imm]` (scale 4) stores
    /// offset / 16. `LDR Q0, [X0]` base = 0x3DC00000 (V=1, opc-high
    /// set, size=0).
    #[test]
    fn pageoff12_scales_ldr128() {
        let ldr_q = 0x3DC0_0000;
        let patched = patch_pageoff12(ldr_q, 0x100);
        assert_eq!((patched >> 10) & 0xFFF, 0x10);
    }

    /// ADD is never treated as a scaled access even when its opcode
    /// bits happen to overlap — `load_store_scale` returns 0.
    #[test]
    fn pageoff12_add_unscaled() {
        assert_eq!(load_store_scale(0x9100_0000), 0);
        let add = 0x9100_0000;
        assert_eq!((patch_pageoff12(add, 0x1F0) >> 10) & 0xFFF, 0x1F0);
    }

    /// A load/store offset not aligned to its access size is a
    /// mis-encode — debug builds must catch it. `LDR X0` needs
    /// 8-aligned offsets; 0x1F4 is not.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "not aligned")]
    fn pageoff12_misaligned_ldr_panics() {
        let _ = patch_pageoff12(0xF940_0000, 0x1F4);
    }

    // ---- write_unsigned64 ----

    #[test]
    fn unsigned64_round_trip_zero() {
        let mut slot = [0xAA; 8];
        write_unsigned64(&mut slot, 0);
        assert_eq!(slot, [0u8; 8]);
    }

    #[test]
    fn unsigned64_round_trip_canonical() {
        let mut slot = [0; 8];
        write_unsigned64(&mut slot, 0x0123_4567_89AB_CDEF);
        // LE byte order.
        assert_eq!(slot, [0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01]);
    }

    #[test]
    fn unsigned64_round_trip_typical_text_segment_base() {
        // The standard MH_EXECUTE __TEXT vmaddr on darwin-arm64.
        let mut slot = [0; 8];
        write_unsigned64(&mut slot, 0x0000_0001_0000_0000);
        assert_eq!(slot, [0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);
    }
}
