//! Tiny LE byte-stream helpers. Every Mach-O field this crate
//! writes goes through one of these — keeping the
//! "host order → little-endian" boundary in one place lets the rest
//! of the writer work in plain `u32` / `u64` and stay easy to
//! diff against `mach-o/loader.h` field-for-field.
//!
//! `to_le_bytes` is the canonical Rust idiom (`<u32 as Numeric>::
//! to_le_bytes`) — these wrappers exist so call sites read like
//! "emit a 32-bit ncmds field" instead of "extend with the LE byte
//! array of a u32".
//!
//! S1 ships `u32_le` only; `u64_le` (symtab offsets) and a
//! pass-through `bytes` writer (section payloads) land alongside
//! the first caller in S2 — no dormant helpers carried as warnings.

pub(crate) fn u32_le(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u32_le_canonical_byte_order() {
        let mut buf = Vec::new();
        u32_le(&mut buf, 0xFEED_FACF);
        // aarch64 Apple Silicon is little-endian, matching Mach-O —
        // 0xFEEDFACF lays out as [CF, FA, ED, FE].
        assert_eq!(buf, [0xCF, 0xFA, 0xED, 0xFE]);
    }
}
