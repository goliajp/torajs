//! `__TEXT,__stubs` per-symbol trampolines for dyld-imported
//! libSystem symbols — SD-2a.
//!
//! Each import gets a 12-byte ARM64 trampoline:
//!
//! ```text
//!   ADRP x16, <la_ptr_page>     ; load page address of slot
//!   LDR  x16, [x16, #pageoff]   ; deref slot → libSystem fn addr
//!   BR   x16                    ; tail-call dyld-bound target
//! ```
//!
//! The slots live in `__DATA,__la_symbol_ptr` (8 B per import,
//! zero-init at link time; dyld fills them at load time via
//! `LC_DYLD_CHAINED_FIXUPS` in SD-3). Until SD-3 wires the chained
//! fixup encoding, the slots stay zero and exec'ing a stub would
//! jump to address 0 — link itself is valid, the binary just isn't
//! runnable for dyld-imported symbols. SD-2a's scope is the link-
//! side substrate; SD-2b wires `stub_vaddrs` into
//! `effective_sym_table` so user-fn reloc sites resolve onto the
//! stub instead of the raw libSystem symbol.
//!
//! Encoding references (ARMv8-A ARM):
//!
//! - **ADRP** — C6.2.10. 21-bit PC-relative page displacement.
//!   Base word `0x9000_0010` = `ADRP X16, #0`; `patch_page21` fills
//!   the immediate.
//! - **LDR (immediate, unsigned offset, 64-bit)** — C6.2.183. Base
//!   word `0xF940_0210` = `LDR X16, [X16]`; the 12-bit immediate
//!   encodes `slot_offset / 8`, so `patch_pageoff12` receives the
//!   scaled value. The slot must be 8-byte aligned (the section
//!   builder enforces this with a `debug_assert`).
//! - **BR Xn** — C6.2.36. Indirect branch with no link. Encoding
//!   for `BR X16` is the fixed word `0xD61F_0200`.

use std::collections::BTreeSet;

use crate::patch::{patch_page21, patch_pageoff12};

/// Each `__TEXT,__stubs` entry is 12 bytes (three ARM64
/// instructions). Apple's textbook `ld64` lays out a 12-byte stub
/// for arm64 — matched here so `Section64.reserved2` and
/// `dyld_imports.len() * STUB_SIZE` line up with what `otool -hv`
/// expects.
pub const STUB_SIZE: u64 = 12;

/// Each `__DATA,__la_symbol_ptr` slot is 8 bytes (one 64-bit
/// pointer; dyld writes the resolved fn address here at bind time).
pub const LA_PTR_SLOT_SIZE: u64 = 8;

/// Base encoding of `ADRP X16, #0`. Opcode bits 31..28 + Xd:
/// `0x90` (= `1_00_10000` ADRP family) at bit 31..24, plus
/// Xd = 16 at bits 4..0 → `0x9000_0010`. immlo (bits 30..29) and
/// immhi (bits 23..5) stay 0; `patch_page21` fills them.
const ADRP_X16_BASE: u32 = 0x9000_0010;

/// Base encoding of `LDR X16, [X16, #0]`. 64-bit LDR (immediate,
/// unsigned offset) opcode bits 31..22 = `0b11_111_001_01` =
/// `0xF94`, Rn=16 at bits 9..5 (`0x10 << 5 = 0x200`), Rt=16 at
/// bits 4..0 (`0x10`). imm12 (bits 21..10) stays 0;
/// `patch_pageoff12` fills it with `slot_offset / 8`.
const LDR_X16_BASE: u32 = 0xF940_0210;

/// Fixed encoding of `BR X16`. ARMv8-A C6.2.36: branch top bits
/// `0xD61F` (bits 31..16), op2 (bits 15..10) = 0, Rn=16 (bits 9..5
/// = `0x10 << 5 = 0x200`), op3 / Rm (bits 4..0) = 0.
const BR_X16: u32 = 0xD61F_0200;

/// One trampoline emitted for a dyld-imported symbol. `bytes` is
/// the on-disk 12-byte payload that goes straight into the
/// `__TEXT,__stubs` section in iteration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stub {
    /// Mangled symbol name — matches what the codegen-side reloc
    /// reference uses (e.g. `_malloc`, `_pthread_create`,
    /// `__tlv_bootstrap`).
    pub name: String,
    /// Three little-endian ARM64 instructions = 12 bytes total.
    pub bytes: [u8; 12],
}

/// Build one `Stub` per import. The vector is in `dyld_imports`
/// iteration order; since `dyld_imports` is a `BTreeSet`, the
/// resulting order is sorted-by-name and reproducible (same input
/// → same output bytes).
///
/// `stubs_section_vaddr` is the final image vaddr of the first
/// byte of `__TEXT,__stubs`. `la_ptr_section_vaddr` is the final
/// image vaddr of the first byte of `__DATA,__la_symbol_ptr`.
/// Per-stub vaddrs / slot vaddrs are derived from these bases:
///
/// ```text
///   stub_i_vaddr = stubs_section_vaddr + i * STUB_SIZE
///   slot_i_vaddr = la_ptr_section_vaddr + i * LA_PTR_SLOT_SIZE
/// ```
///
/// Each stub's three instructions then resolve to:
///
/// ```text
///   ADRP X16, page_of(slot_i_vaddr)
///       page_disp = (slot_i_vaddr & ~0xFFF) - (stub_i_vaddr & ~0xFFF)
///   LDR  X16, [X16, slot_i_vaddr & 0xFFF]
///   BR   X16
/// ```
///
/// Panics in debug builds if `la_ptr_section_vaddr` is not
/// 8-byte aligned — the slot scale of `LDR (immediate, 64-bit
/// unsigned offset)` requires the byte offset to be a multiple of
/// 8 (the encoded `imm12` is `byte_offset / 8`).
pub fn build_stubs(
    dyld_imports: &BTreeSet<String>,
    stubs_section_vaddr: u64,
    la_ptr_section_vaddr: u64,
) -> Vec<Stub> {
    let mut stubs = Vec::with_capacity(dyld_imports.len());
    for (i, name) in dyld_imports.iter().enumerate() {
        let stub_vaddr = stubs_section_vaddr + (i as u64) * STUB_SIZE;
        let slot_vaddr = la_ptr_section_vaddr + (i as u64) * LA_PTR_SLOT_SIZE;
        let slot_page = (slot_vaddr & !0xFFF) as i64;
        let stub_page = (stub_vaddr & !0xFFF) as i64;
        let page_disp = slot_page - stub_page;
        let pageoff_bytes = (slot_vaddr & 0xFFF) as u32;
        debug_assert_eq!(
            pageoff_bytes % 8,
            0,
            "la_ptr slot pageoff {pageoff_bytes:#X} must be 8-byte aligned for 64-bit LDR imm12"
        );

        let adrp = patch_page21(ADRP_X16_BASE, page_disp);
        let ldr = patch_pageoff12(LDR_X16_BASE, pageoff_bytes / 8);

        let mut bytes = [0u8; 12];
        bytes[0..4].copy_from_slice(&adrp.to_le_bytes());
        bytes[4..8].copy_from_slice(&ldr.to_le_bytes());
        bytes[8..12].copy_from_slice(&BR_X16.to_le_bytes());

        stubs.push(Stub {
            name: name.clone(),
            bytes,
        });
    }
    stubs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imports_of(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    /// Empty `dyld_imports` → empty stub vector. Matches the
    /// SD-1 "syscall path" where no libSystem symbols are
    /// referenced and the binary stays byte-identical.
    #[test]
    fn empty_imports_emits_no_stubs() {
        let stubs = build_stubs(&BTreeSet::new(), 0x1_0000_4000, 0x1_0000_8000);
        assert!(stubs.is_empty());
    }

    /// One import → one 12-byte stub carrying the import's name.
    #[test]
    fn single_import_emits_one_12_byte_stub() {
        let imports = imports_of(&["_malloc"]);
        let stubs = build_stubs(&imports, 0x1_0000_8000, 0x1_0001_0000);
        assert_eq!(stubs.len(), 1);
        assert_eq!(stubs[0].name, "_malloc");
        assert_eq!(stubs[0].bytes.len(), 12);
    }

    /// Two imports → 24 bytes total. `BTreeSet` iter order is
    /// sorted-by-name, so the output is reproducible regardless of
    /// insertion order.
    #[test]
    fn two_imports_emit_24_bytes_sorted_by_name() {
        let imports = imports_of(&["_z_last", "_a_first"]);
        let stubs = build_stubs(&imports, 0x1_0000_8000, 0x1_0001_0000);
        assert_eq!(stubs.len(), 2);
        assert_eq!(stubs[0].name, "_a_first");
        assert_eq!(stubs[1].name, "_z_last");
        let total: usize = stubs.iter().map(|s| s.bytes.len()).sum();
        assert_eq!(total, 24);
    }

    /// ADRP imm21 round-trip. Stubs at 0x100008000, slots at
    /// 0x100010000 → page displacement = 0x10000 - 0x8000 = 0x8000
    /// → imm21 = 0x8.
    #[test]
    fn adrp_page_displacement_round_trips() {
        let imports = imports_of(&["_x"]);
        let stubs = build_stubs(&imports, 0x1_0000_8000, 0x1_0001_0000);
        let adrp = u32::from_le_bytes(stubs[0].bytes[0..4].try_into().unwrap());
        // immlo at bits 30..29, immhi at bits 23..5; reconstruct
        // the 21-bit unsigned value.
        let immlo = (adrp >> 29) & 0b11;
        let immhi = (adrp >> 5) & 0x7_FFFF;
        let imm21 = (immhi << 2) | immlo;
        assert_eq!(imm21, 0x8);
    }

    /// LDR imm12 round-trip. Slot at 0x100010100 → pageoff 0x100
    /// → imm12 = 0x100 / 8 = 0x20.
    #[test]
    fn ldr_pageoff_round_trips() {
        let imports = imports_of(&["_x"]);
        let stubs = build_stubs(&imports, 0x1_0000_8000, 0x1_0001_0100);
        let ldr = u32::from_le_bytes(stubs[0].bytes[4..8].try_into().unwrap());
        let imm12 = (ldr >> 10) & 0xFFF;
        assert_eq!(imm12, 0x100 / 8);
    }

    /// The third instruction is the unmodified `BR X16` opcode.
    #[test]
    fn br_x16_is_third_instruction() {
        let imports = imports_of(&["_x"]);
        let stubs = build_stubs(&imports, 0x1_0000_8000, 0x1_0001_0000);
        let br = u32::from_le_bytes(stubs[0].bytes[8..12].try_into().unwrap());
        assert_eq!(br, BR_X16);
    }

    /// Stubs + slots in the same 4 KiB page → ADRP imm21 = 0
    /// (page displacement = 0) → emitted ADRP word stays at the
    /// base encoding.
    #[test]
    fn same_page_stubs_emit_zero_adrp_immediate() {
        let imports = imports_of(&["_x"]);
        let stubs = build_stubs(&imports, 0x1_0000_4000, 0x1_0000_4FF8);
        let adrp = u32::from_le_bytes(stubs[0].bytes[0..4].try_into().unwrap());
        assert_eq!(adrp, ADRP_X16_BASE);
    }

    /// Per-stub vaddr stride = `STUB_SIZE` (12). For two imports the
    /// second stub's ADRP must compute its page displacement against
    /// `stubs_section_vaddr + STUB_SIZE`, not the section base. The
    /// LDR imm12 must walk by `LA_PTR_SLOT_SIZE` (8) per import.
    #[test]
    fn second_stub_uses_stride_aware_vaddrs() {
        let imports = imports_of(&["_a", "_b"]);
        // Choose vaddrs so that stub[0] and slot[0] share the
        // same page, while stub[1]'s page differs from slot[1]'s
        // by exactly one — this isolates the stride math from
        // base-only addressing.
        //
        // stub[0] = 0x100004000, slot[0] = 0x100004008
        // stub[1] = 0x10000400C, slot[1] = 0x100004010
        // Both stubs land in page 0x100004000, both slots too →
        // page displacement = 0 for both, but LDR pageoff differs:
        //   stub[0] LDR pageoff = 0x008 → imm12 = 1
        //   stub[1] LDR pageoff = 0x010 → imm12 = 2
        let stubs = build_stubs(&imports, 0x1_0000_4000, 0x1_0000_4008);
        let ldr0 = u32::from_le_bytes(stubs[0].bytes[4..8].try_into().unwrap());
        let ldr1 = u32::from_le_bytes(stubs[1].bytes[4..8].try_into().unwrap());
        assert_eq!((ldr0 >> 10) & 0xFFF, 1, "stub[0] imm12");
        assert_eq!((ldr1 >> 10) & 0xFFF, 2, "stub[1] imm12");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "must be 8-byte aligned")]
    fn unaligned_la_ptr_section_panics() {
        let imports = imports_of(&["_x"]);
        // slot pageoff = 0x004 → not divisible by 8.
        let _ = build_stubs(&imports, 0x1_0000_8000, 0x1_0001_0004);
    }
}
