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
//! S1 shipped `u32_le` only. S2 lights up `u64_le` (segment
//! vmaddr / vmsize / fileoff / filesize fields and section addr /
//! size) and the pass-through `bytes` writer (the 16-byte
//! `segname` / `sectname` zero-padded char arrays from
//! `mach-o/loader.h`).

pub(crate) fn u32_le(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn u64_le(buf: &mut Vec<u8>, value: u64) {
    buf.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn bytes(buf: &mut Vec<u8>, value: &[u8]) {
    buf.extend_from_slice(value);
}

/// Encode a Mach-O 16-byte `segname` / `sectname` slot: copy `name`
/// (UTF-8, must be ≤ 16 bytes) and zero-pad to exactly 16 bytes.
/// Panics on overflow — Apple's loader.h fixes the slot size so a
/// silent truncation would corrupt the on-disk layout.
pub(crate) fn name16(value: &str) -> [u8; 16] {
    let src = value.as_bytes();
    assert!(
        src.len() <= 16,
        "Mach-O segname/sectname must be ≤ 16 bytes; got {} for {value:?}",
        src.len()
    );
    let mut out = [0u8; 16];
    out[..src.len()].copy_from_slice(src);
    out
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

    #[test]
    fn u64_le_canonical_byte_order() {
        let mut buf = Vec::new();
        u64_le(&mut buf, 0x0123_4567_89AB_CDEF);
        assert_eq!(buf, [0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01]);
    }

    #[test]
    fn bytes_pass_through() {
        let mut buf = Vec::from([0xAA, 0xBB]);
        bytes(&mut buf, &[0xCC, 0xDD]);
        assert_eq!(buf, [0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn name16_zero_pads_short_name() {
        // "__TEXT" is 6 bytes, slot fills with 10 trailing NULs.
        assert_eq!(
            name16("__TEXT"),
            [
                b'_', b'_', b'T', b'E', b'X', b'T', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );
    }

    #[test]
    fn name16_fills_exact_16() {
        // Exactly 16 bytes — no trailing NUL slot in Mach-O's
        // fixed-width segname/sectname field.
        let n = name16("ABCDEFGHIJKLMNOP");
        assert_eq!(&n[..], b"ABCDEFGHIJKLMNOP");
    }

    #[test]
    #[should_panic(expected = "must be ≤ 16 bytes")]
    fn name16_overflow_panics() {
        let _ = name16("0123456789ABCDEFG"); // 17 bytes
    }
}
