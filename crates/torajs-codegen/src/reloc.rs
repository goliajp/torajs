//! Relocation descriptors.
//!
//! Codegen emits raw instruction bytes plus a `Vec<Reloc>` per
//! function. Each `Reloc` describes a site whose final 32-bit (or
//! larger) value must be patched after link-time addresses are known.
//! `torajs-obj` (#7) consumes these to emit the matching Mach-O
//! R_ARM64_* / ELF R_AARCH64_* relocation entries; `torajs-link` (#8)
//! ultimately resolves them.
//!
//! S1 has no relocs (`1 + 2` is leaf + no globals). The types exist
//! so S2-S5 can append without reshuffling the API.

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
    /// 26-bit signed PC-relative branch displacement. Used by `BL`
    /// (call site). torajs-link writes `(target_addr - site_addr) / 4`
    /// into the low 26 bits, sign-extended.
    CallSite { target_func: FuncId },

    /// ADRP + ADD pair, page-aligned address of a data symbol.
    /// The pair always occupies two consecutive instructions; the
    /// `byte_offset` here points to the ADRP. ADD imm12 is patched
    /// from the same target.
    DataAdr { target_sym: String },

    /// 64-bit absolute address. Used when storing a fn pointer or
    /// global to memory (e.g. vtable slot). The 8 bytes at
    /// `byte_offset` are overwritten with the resolved address.
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
            byte_offset: 4,
            kind: RelocKind::DataAdr {
                target_sym: "__torajs_str_literal_0".into(),
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
