//! Mach-O `relocation_info` + the ARM64-specific reloc type
//! taxonomy from `<mach-o/arm64/reloc.h>`.
//!
//! Wire format per `<mach-o/reloc.h>`:
//!
//! ```text
//!   struct relocation_info {              // 8 bytes total
//!       int32_t  r_address;               // offset in the section
//!       uint32_t r_symbolnum : 24;        // bits  0..23
//!       uint32_t r_pcrel     : 1;         // bit   24
//!       uint32_t r_length    : 2;         // bits 25..26
//!       uint32_t r_extern    : 1;         // bit   27
//!       uint32_t r_type      : 4;         // bits 28..31
//!   };
//! ```
//!
//! On-disk: `r_address` is a 4-byte little-endian word followed by
//! the 4-byte packed bitfield word, also little-endian.
//!
//! `r_length` encodes `log2(byte_size)` — 2 = 4 bytes (every
//! aarch64 instruction reloc patches a 4-byte slot) and 3 = 8
//! bytes (the abs-64 pointer in a data section).
//!
//! `r_extern = 1` (every reloc this writer emits is symbol-keyed,
//! not section-keyed) means `r_symbolnum` indexes the `nlist_64`
//! table built by `symtab.rs`.

use crate::write::u32_le;

/// ARM64 reloc type — 64-bit absolute pointer in a data section.
/// `r_length = 3` (8 bytes), `r_pcrel = 0`.
/// Mirrors `ARM64_RELOC_UNSIGNED`.
pub const ARM64_RELOC_UNSIGNED: u8 = 0;

/// ARM64 reloc type — 26-bit PC-relative branch displacement
/// (BL site). `r_length = 2`, `r_pcrel = 1`.
/// Mirrors `ARM64_RELOC_BRANCH26`.
pub const ARM64_RELOC_BRANCH26: u8 = 2;

/// ARM64 reloc type — 21-bit page-relative distance for ADRP.
/// `r_length = 2`, `r_pcrel = 1`. Always paired with a
/// `ARM64_RELOC_PAGEOFF12` on the matching ADD.
/// Mirrors `ARM64_RELOC_PAGE21`.
pub const ARM64_RELOC_PAGE21: u8 = 3;

/// ARM64 reloc type — 12-bit page offset for ADD (imm).
/// `r_length = 2`, `r_pcrel = 0`.
/// Mirrors `ARM64_RELOC_PAGEOFF12`.
pub const ARM64_RELOC_PAGEOFF12: u8 = 4;

/// ARM64 reloc type — 21-bit page-relative distance for ADRP that
/// loads from a GOT slot. `r_length = 2`, `r_pcrel = 1`. The
/// instruction at the patch site is an `ADRP Xn, _sym@GOTPAGE` —
/// our linker performs a textbook ld64-style relaxation, treating
/// it identically to `ARM64_RELOC_PAGE21` (direct page disp to
/// the symbol, no GOT indirection) when the target sits within
/// the binary's ±4 GiB image range. Mirrors
/// `ARM64_RELOC_GOT_LOAD_PAGE21`.
pub const ARM64_RELOC_GOT_LOAD_PAGE21: u8 = 5;

/// ARM64 reloc type — 12-bit page offset for LDR that loads a GOT
/// slot. `r_length = 2`, `r_pcrel = 0`. Paired with
/// `ARM64_RELOC_GOT_LOAD_PAGE21`. Our linker relaxes the LDR
/// instruction to an ADD instruction in place and patches the imm12
/// to the symbol's page-offset — turning `ADRP+LDR` (indirect via
/// GOT) into `ADRP+ADD` (PC-relative pointer to the symbol
/// itself). Mirrors `ARM64_RELOC_GOT_LOAD_PAGEOFF12`.
pub const ARM64_RELOC_GOT_LOAD_PAGEOFF12: u8 = 6;

/// ARM64 reloc type — `ADRP Xn, _sym@TLVPPAGE`. Loads the page
/// portion of a TLV (thread-local variable) descriptor. `r_length
/// = 2`, `r_pcrel = 1`. The descriptor lives in `__DATA,__thread_vars`
/// and contains `{ tlv_addr_resolver, key, offset }`. Mirrors
/// `ARM64_RELOC_TLVP_LOAD_PAGE21`.
pub const ARM64_RELOC_TLVP_LOAD_PAGE21: u8 = 8;

/// ARM64 reloc type — `LDR Xn, [Xn, _sym@TLVPPAGEOFF]`. Loads the
/// TLV descriptor pointer from the page identified by the paired
/// `ARM64_RELOC_TLVP_LOAD_PAGE21`. `r_length = 2`, `r_pcrel = 0`.
/// Mirrors `ARM64_RELOC_TLVP_LOAD_PAGEOFF12`.
pub const ARM64_RELOC_TLVP_LOAD_PAGEOFF12: u8 = 9;

/// ARM64 reloc type — meta-reloc carrying a 24-bit signed addend.
/// Its `r_symbolnum` field holds the addend value (sign-extended
/// from 24 to 32 bits); the addend then applies to the *next*
/// reloc entry in the table (typically a paired PAGE21 +
/// PAGEOFF12). `r_extern = 0` by convention. Mirrors
/// `ARM64_RELOC_ADDEND`.
pub const ARM64_RELOC_ADDEND: u8 = 10;

/// On-disk size of one `relocation_info` entry.
pub const RELOCATION_INFO_SIZE: u32 = 8;

/// Mirror of `relocation_info`. Field names match the C struct so
/// the on-disk bit-pack stays auditable against
/// `<mach-o/reloc.h>` field-for-field.
///
/// Constraints:
///   - `r_symbolnum` ≤ `0xFF_FFFF` (24-bit slot).
///   - `r_pcrel` ∈ `{0, 1}`.
///   - `r_length` ∈ `{0, 1, 2, 3}` = log2(byte size).
///   - `r_extern` ∈ `{0, 1}`.
///   - `r_type` ∈ `{0..15}`.
///
/// All builder helpers below enforce these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelocationInfo {
    pub r_address: u32,
    pub r_symbolnum: u32,
    pub r_pcrel: u8,
    pub r_length: u8,
    pub r_extern: u8,
    pub r_type: u8,
}

impl RelocationInfo {
    /// 4-byte BL branch reloc — `ARM64_RELOC_BRANCH26`. `r_length`
    /// = 2 (4-byte instruction word), `r_pcrel` = 1, `r_extern` =
    /// 1. `r_address` is the byte offset within `__TEXT,__text`
    /// where the BL instruction sits.
    pub fn branch26(r_address: u32, sym_idx: u32) -> Self {
        Self::new_extern(r_address, sym_idx, ARM64_RELOC_BRANCH26, 1, 2)
    }

    /// 4-byte ADRP page-21 reloc — `ARM64_RELOC_PAGE21`.
    /// `r_length` = 2, `r_pcrel` = 1, `r_extern` = 1.
    pub fn page21(r_address: u32, sym_idx: u32) -> Self {
        Self::new_extern(r_address, sym_idx, ARM64_RELOC_PAGE21, 1, 2)
    }

    /// 4-byte ADD page-off-12 reloc — `ARM64_RELOC_PAGEOFF12`.
    /// `r_length` = 2, `r_pcrel` = 0, `r_extern` = 1. Always
    /// follows a `page21` on the matching ADD instruction.
    pub fn pageoff12(r_address: u32, sym_idx: u32) -> Self {
        Self::new_extern(r_address, sym_idx, ARM64_RELOC_PAGEOFF12, 0, 2)
    }

    /// 8-byte absolute pointer reloc — `ARM64_RELOC_UNSIGNED`.
    /// `r_length` = 3 (8 bytes), `r_pcrel` = 0, `r_extern` = 1.
    /// Used in data sections (vtable slots, static fn ptrs).
    pub fn unsigned64(r_address: u32, sym_idx: u32) -> Self {
        Self::new_extern(r_address, sym_idx, ARM64_RELOC_UNSIGNED, 0, 3)
    }

    fn new_extern(r_address: u32, sym_idx: u32, r_type: u8, r_pcrel: u8, r_length: u8) -> Self {
        assert!(
            sym_idx <= 0xFF_FFFF,
            "r_symbolnum must fit in 24 bits; got 0x{sym_idx:08x}"
        );
        Self {
            r_address,
            r_symbolnum: sym_idx,
            r_pcrel,
            r_length,
            r_extern: 1,
            r_type,
        }
    }

    /// Append the 8-byte little-endian on-disk form to `buf`.
    pub fn write_to(&self, buf: &mut Vec<u8>) {
        let start = buf.len();
        u32_le(buf, self.r_address);
        u32_le(buf, self.packed_word());
        debug_assert_eq!(
            (buf.len() - start) as u32,
            RELOCATION_INFO_SIZE,
            "relocation_info must serialize to exactly 8 bytes"
        );
    }

    /// Pack `r_symbolnum` (24 bits) | `r_pcrel` (1) | `r_length`
    /// (2) | `r_extern` (1) | `r_type` (4) into one 32-bit word
    /// per `<mach-o/reloc.h>` bit layout.
    fn packed_word(&self) -> u32 {
        debug_assert!(self.r_symbolnum <= 0xFF_FFFF);
        debug_assert!(self.r_pcrel <= 1);
        debug_assert!(self.r_length <= 3);
        debug_assert!(self.r_extern <= 1);
        debug_assert!(self.r_type <= 0xF);
        (self.r_symbolnum & 0xFF_FFFF)
            | (u32::from(self.r_pcrel) << 24)
            | (u32::from(self.r_length) << 25)
            | (u32::from(self.r_extern) << 27)
            | (u32::from(self.r_type) << 28)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch26_packs_correct_word() {
        // BL site at byte 0x10, symbol 0x05.
        // Expected packed word:
        //   r_symbolnum = 0x000005
        //   r_pcrel     = 1   << 24 = 0x0100_0000
        //   r_length    = 2   << 25 = 0x0400_0000
        //   r_extern    = 1   << 27 = 0x0800_0000
        //   r_type      = 2   << 28 = 0x2000_0000
        //   ───────────────────────  0x2D00_0005
        let r = RelocationInfo::branch26(0x10, 0x05);
        let mut buf = Vec::new();
        r.write_to(&mut buf);
        assert_eq!(buf.len(), 8);
        // r_address @ 0..4 = 0x10
        assert_eq!(&buf[0..4], &[0x10, 0x00, 0x00, 0x00]);
        // packed @ 4..8 = 0x2D00_0005 LE → [05, 00, 00, 2D]
        assert_eq!(&buf[4..8], &[0x05, 0x00, 0x00, 0x2D]);
    }

    #[test]
    fn page21_packs_correct_word() {
        // ADRP site at byte 0x20, symbol 0x03.
        //   r_type      = PAGE21 = 3 << 28 = 0x3000_0000
        //   r_extern    = 1     << 27 = 0x0800_0000
        //   r_length    = 2     << 25 = 0x0400_0000
        //   r_pcrel     = 1     << 24 = 0x0100_0000
        //   r_symbolnum = 0x000003
        //   ─────────────────────────── 0x3D00_0003
        let r = RelocationInfo::page21(0x20, 0x03);
        let mut buf = Vec::new();
        r.write_to(&mut buf);
        assert_eq!(&buf[0..4], &[0x20, 0x00, 0x00, 0x00]);
        assert_eq!(&buf[4..8], &[0x03, 0x00, 0x00, 0x3D]);
    }

    #[test]
    fn pageoff12_packs_correct_word() {
        // ADD site at byte 0x24, symbol 0x03 — r_pcrel = 0.
        //   r_type      = PAGEOFF12 = 4 << 28 = 0x4000_0000
        //   r_extern    = 1         << 27 = 0x0800_0000
        //   r_length    = 2         << 25 = 0x0400_0000
        //   r_pcrel     = 0
        //   r_symbolnum = 0x000003
        //   ─────────────────────────────── 0x4C00_0003
        let r = RelocationInfo::pageoff12(0x24, 0x03);
        let mut buf = Vec::new();
        r.write_to(&mut buf);
        assert_eq!(&buf[0..4], &[0x24, 0x00, 0x00, 0x00]);
        assert_eq!(&buf[4..8], &[0x03, 0x00, 0x00, 0x4C]);
    }

    #[test]
    fn unsigned64_packs_correct_word() {
        // 8-byte data ptr at byte 0x00, symbol 0x07.
        //   r_type      = UNSIGNED = 0 << 28 = 0
        //   r_extern    = 1        << 27 = 0x0800_0000
        //   r_length    = 3        << 25 = 0x0600_0000
        //   r_pcrel     = 0
        //   r_symbolnum = 0x000007
        //   ─────────────────────────────── 0x0E00_0007
        let r = RelocationInfo::unsigned64(0x00, 0x07);
        let mut buf = Vec::new();
        r.write_to(&mut buf);
        assert_eq!(&buf[0..4], &[0x00, 0x00, 0x00, 0x00]);
        assert_eq!(&buf[4..8], &[0x07, 0x00, 0x00, 0x0E]);
    }

    #[test]
    fn struct_sizes_match_apple_reloc_h() {
        assert_eq!(RELOCATION_INFO_SIZE, 8);
        assert_eq!(ARM64_RELOC_UNSIGNED, 0);
        assert_eq!(ARM64_RELOC_BRANCH26, 2);
        assert_eq!(ARM64_RELOC_PAGE21, 3);
        assert_eq!(ARM64_RELOC_PAGEOFF12, 4);
    }

    #[test]
    fn r_symbolnum_max_24_bit_value_round_trips() {
        // 0xFFFFFF — the largest legal r_symbolnum.
        let r = RelocationInfo::branch26(0, 0xFF_FFFF);
        let mut buf = Vec::new();
        r.write_to(&mut buf);
        // packed = 0xFFFFFF | (1<<24) | (2<<25) | (1<<27) | (2<<28)
        //        = 0x00FF_FFFF | 0x2D00_0000 = 0x2DFF_FFFF
        // LE → [FF, FF, FF, 2D]
        assert_eq!(&buf[4..8], &[0xFF, 0xFF, 0xFF, 0x2D]);
    }

    #[test]
    #[should_panic(expected = "r_symbolnum must fit in 24 bits")]
    fn r_symbolnum_overflow_panics() {
        // 0x100_0000 = 25 bits set — silent truncation would
        // corrupt the bitfield, so the builder must reject.
        let _ = RelocationInfo::branch26(0, 0x100_0000);
    }
}
