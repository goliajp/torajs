//! Relocation descriptors.
//!
//! Codegen emits raw instruction bytes plus a `Vec<Reloc>` per
//! function. Each `Reloc` describes a site whose final 32-bit (or
//! larger) value must be patched after link-time addresses are known.
//! `torajs-obj` (#7) consumes these to emit the matching Mach-O
//! R_ARM64_* / ELF R_AARCH64_* relocation entries; `torajs-link` (#8)
//! ultimately resolves them.
//!
//! Kinds mirror the Mach-O / ELF aarch64 reloc taxonomy 1:1:
//!
//!   - `CallSite`   → ARM64_RELOC_BRANCH26 / R_AARCH64_CALL26
//!   - `Page21`     → ARM64_RELOC_PAGE21   / R_AARCH64_ADR_PREL_PG_HI21
//!   - `PageOff12`  → ARM64_RELOC_PAGEOFF12 / R_AARCH64_ADD_ABS_LO12_NC
//!   - `AbsPtr64`   → ARM64_RELOC_UNSIGNED  / R_AARCH64_ABS64
//!
//! S4-A populates `CallSite`. S4-B (this commit) populates
//! `Page21` + `PageOff12` for GlobalRef / StringRef / StaticStrRef /
//! FnAddr. `AbsPtr64` lives for the data section (vtable slots etc),
//! emitted by `torajs-obj` rather than `torajs-codegen` body lowering.

use torajs_core::ssa::FuncId;

#[derive(Debug, Clone)]
pub struct Reloc {
    /// Byte offset within the function's compiled bytes where the
    /// reloc applies. Always a multiple of 4 (aarch64 fixed-width
    /// instructions).
    pub byte_offset: u32,
    pub kind: RelocKind,
}

#[derive(Debug, Clone)]
pub enum RelocKind {
    /// 26-bit signed PC-relative branch displacement (BL site).
    /// torajs-link writes `(target_addr - site_addr) / 4` into the
    /// low 26 bits, sign-extended.
    CallSite { target_func: FuncId },

    /// ARM64 PAGE21 — fills the 21-bit `imm21` field of an ADRP
    /// instruction with the page (4 KiB-aligned) address of
    /// `target_sym` relative to the ADRP's PC.
    Page21 { target_sym: String },

    /// ARM64 PAGEOFF12 — fills the 12-bit `imm12` field of an ADD
    /// (immediate) with the low 12 bits of `target_sym`'s page
    /// offset. Always paired with a `Page21` at PC-4.
    PageOff12 { target_sym: String },

    /// 64-bit absolute address — fills 8 bytes with the resolved
    /// address of `target_sym`. Used in data sections (vtable
    /// slots, static fn ptrs).
    AbsPtr64 { target_sym: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reloc_kinds_are_constructible() {
        let _ = Reloc {
            byte_offset: 0,
            kind: RelocKind::CallSite {
                target_func: FuncId(0),
            },
        };
        let _ = Reloc {
            byte_offset: 0,
            kind: RelocKind::Page21 {
                target_sym: "__torajs_str_lit_0".into(),
            },
        };
        let _ = Reloc {
            byte_offset: 4,
            kind: RelocKind::PageOff12 {
                target_sym: "__torajs_str_lit_0".into(),
            },
        };
        let _ = Reloc {
            byte_offset: 8,
            kind: RelocKind::AbsPtr64 {
                target_sym: "vtable_Foo".into(),
            },
        };
    }
}
