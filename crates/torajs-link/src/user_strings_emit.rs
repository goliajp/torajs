//! SD-4c-prereq+e1 — emit pass for user-binary string literals.
//!
//! Materializes the packed `[u64 header, u32 length, u32 _pad,
//! [N x i8] bytes]` rodata Str layout per `UserStringEntry`,
//! producing one contiguous byte buffer ready to splice into
//! the `__TEXT,__cstring` region the user-strings layout pass
//! placed. See [`crate::user_strings_layout`] for the byte ABI
//! reference vs `ssa_inkwell::globals::emit_static_str_global`.
//!
//! Also: `apply_user_string_overrides` registers each entry's
//! `sym → vaddr` into the effective sym table the linker's
//! `apply_relocs` pass consumes — codegen-emitted
//! `RelocKind::Page21 / PageOff12 { target_sym: e.sym }` ADRP+ADD
//! pairs resolve through this map.

use crate::exec::{UserStringEntry, UserStringKind};
use crate::resolve::SymTable;
use crate::user_strings_layout::{STATIC_LITERAL_FLAG, STR_FLAG_IS_LATIN1, UserStringsLayout};

/// Build the contiguous user-strings byte payload for one
/// `LinkConfig.strings` set. Caller is responsible for splicing
/// the returned buffer at `layout.file_offset` (= directly after
/// the member non-text payloads / before `__TEXT,__stubs`).
/// Returns an empty Vec for `strings.is_empty()` — caller's
/// `extend_from_slice` is a no-op so the byte stream stays
/// pre-e1 identical.
pub fn build_user_strings_payload(
    strings: &[UserStringEntry],
    layout: &UserStringsLayout,
) -> Vec<u8> {
    debug_assert_eq!(
        strings.len(),
        layout.entries.len(),
        "strings and layout.entries must zip 1:1"
    );
    let mut buf = Vec::with_capacity(layout.total_size as usize);
    for (e, le) in strings.iter().zip(layout.entries.iter()) {
        match e.kind {
            UserStringKind::StaticStr => {
                let mut flags: u16 = STATIC_LITERAL_FLAG;
                if e.is_latin1 {
                    flags |= STR_FLAG_IS_LATIN1;
                }
                // refcount=1 in low 32 bits; type_tag=TAG_STR=0 in
                // [32..48]; flags in [48..64]. Mirrors emit_static_str_global
                // (refcount is irrelevant — STATIC bit short-circuits
                // rc_inc/dec at runtime).
                let header_u64: u64 = 1u64 | ((flags as u64) << 48);
                buf.extend_from_slice(&header_u64.to_le_bytes());
                buf.extend_from_slice(&e.length.to_le_bytes());
                buf.extend_from_slice(&[0u8; 4]); // _pad
                buf.extend_from_slice(&e.bytes);
            }
            UserStringKind::RawBytes => {
                // Mirrors emit_string_global — payload bytes only, no
                // header. Caller side (`__torajs_str_alloc(ptr, length)`)
                // passes the length separately at the SSA layer.
                buf.extend_from_slice(&e.bytes);
            }
        }
        debug_assert_eq!(
            buf.len() as u32 - (le.file_offset - layout.file_offset),
            le.payload_size,
            "emitted entry size must match layout.payload_size"
        );
    }
    // Trailing zero-pad to 4-byte alignment so the next __TEXT section
    // (`__stubs`, ARM64 instruction stream) lands aligned.
    while (buf.len() as u32) < layout.total_size {
        buf.push(0);
    }
    debug_assert_eq!(buf.len() as u32, layout.total_size);
    buf
}

/// Register each entry's `sym → vaddr` in the effective sym table.
/// Follows the same shape as `apply_tlv_overrides` (TLV thunk-slot
/// sym overrides) — caller invokes after `effective_sym_table`
/// already absorbed member `defined_syms` + stub vaddrs.
pub fn apply_user_string_overrides(layout: &UserStringsLayout, sym_table: &mut SymTable) {
    for entry in &layout.entries {
        sym_table.insert(entry.sym.clone(), entry.vaddr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user_strings_layout::{STR_HEADER_SIZE, compute_user_strings_layout};

    fn make_entry(sym: &str, bytes: &[u8], is_latin1: bool, length: u32) -> UserStringEntry {
        UserStringEntry {
            sym: sym.into(),
            bytes: bytes.to_vec(),
            is_latin1,
            length,
            kind: UserStringKind::StaticStr,
        }
    }

    fn make_raw_entry(sym: &str, bytes: &[u8]) -> UserStringEntry {
        UserStringEntry {
            sym: sym.into(),
            bytes: bytes.to_vec(),
            is_latin1: true,
            length: bytes.len() as u32,
            kind: UserStringKind::RawBytes,
        }
    }

    #[test]
    fn empty_strings_emits_empty_buf() {
        let layout = compute_user_strings_layout(&[], 0x4000, 0x1_0000_4000);
        let buf = build_user_strings_payload(&[], &layout);
        assert!(buf.is_empty());
    }

    #[test]
    fn single_latin1_entry_byte_pattern() {
        let strings = [make_entry("__torajs_str_lit_0", b"hello", true, 5)];
        let layout = compute_user_strings_layout(&strings, 0x4000, 0x1_0000_4000);
        let buf = build_user_strings_payload(&strings, &layout);
        // Raw = 16 header + 5 payload = 21, rounded up to 24 (zero
        // pad keeps the next __TEXT section 4-byte aligned).
        assert_eq!(buf.len() as u32, 24);
        // Trailing 3 bytes are zero pad.
        assert_eq!(&buf[21..24], &[0u8; 3]);

        // Header LE: refcount=1 (low 32) | flags<<48.
        //   flags = STATIC_LITERAL_FLAG | STR_FLAG_IS_LATIN1 = 4 | 2 = 6.
        let expected_header: u64 = 1 | (6u64 << 48);
        let header_le = u64::from_le_bytes(buf[0..8].try_into().unwrap());
        assert_eq!(header_le, expected_header);

        // Length u32 @8 = 5.
        let length_le = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        assert_eq!(length_le, 5);

        // Pad u32 @12 = 0.
        let pad_le = u32::from_le_bytes(buf[12..16].try_into().unwrap());
        assert_eq!(pad_le, 0);

        // Payload @16 = "hello".
        assert_eq!(&buf[16..21], b"hello");
    }

    #[test]
    fn utf16_entry_clears_latin1_bit() {
        let strings = [make_entry(
            "__torajs_str_lit_0",
            &[0x68, 0x00, 0x69, 0x00],
            false,
            2,
        )];
        let layout = compute_user_strings_layout(&strings, 0x4000, 0x1_0000_4000);
        let buf = build_user_strings_payload(&strings, &layout);

        // flags = STATIC_LITERAL_FLAG only (no IS_LATIN1) = 4.
        let expected_header: u64 = 1 | (4u64 << 48);
        let header_le = u64::from_le_bytes(buf[0..8].try_into().unwrap());
        assert_eq!(header_le, expected_header);

        // Length = 2 (code units), payload = 4 bytes.
        let length_le = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        assert_eq!(length_le, 2);
        assert_eq!(&buf[16..20], &[0x68, 0x00, 0x69, 0x00]);
    }

    #[test]
    fn multi_entry_payloads_concat_back_to_back() {
        let strings = [
            make_entry("__torajs_str_lit_0", b"hi", true, 2),
            make_entry("__torajs_str_lit_1", b"yo", true, 2),
        ];
        let layout = compute_user_strings_layout(&strings, 0x4000, 0x1_0000_4000);
        let buf = build_user_strings_payload(&strings, &layout);

        let entry_size = (STR_HEADER_SIZE + 2) as usize;
        assert_eq!(buf.len(), entry_size * 2);
        // First entry payload @16 = "hi".
        assert_eq!(&buf[16..18], b"hi");
        // Second entry payload @(entry_size + 16) = "yo".
        assert_eq!(&buf[entry_size + 16..entry_size + 18], b"yo");
    }

    #[test]
    fn raw_bytes_entry_emits_payload_only() {
        let strings = [make_raw_entry("__torajs_str_dyn_0", b"hello")];
        let layout = compute_user_strings_layout(&strings, 0x4000, 0x1_0000_4000);
        let buf = build_user_strings_payload(&strings, &layout);
        // 5 payload bytes, then 3 zero pad bytes to 4-byte align.
        assert_eq!(buf.len(), 8);
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(&buf[5..8], &[0u8; 3]);
    }

    #[test]
    fn mixed_kinds_concat_static_then_raw() {
        // Static (16+5=21) + RawBytes (3) = 24, already 4-aligned.
        let strings = [
            make_entry("__torajs_str_lit_0", b"hello", true, 5),
            make_raw_entry("__torajs_str_dyn_0", b"abc"),
        ];
        let layout = compute_user_strings_layout(&strings, 0x4000, 0x1_0000_4000);
        let buf = build_user_strings_payload(&strings, &layout);
        assert_eq!(buf.len(), 24);
        // Static entry: header + length + pad + "hello" @ [0..21].
        assert_eq!(&buf[16..21], b"hello");
        // RawBytes entry starts at offset 21 — payload only.
        assert_eq!(&buf[21..24], b"abc");
    }

    #[test]
    fn apply_user_string_overrides_inserts_sym_vaddr() {
        let strings = [
            make_entry("__torajs_str_lit_0", b"hi", true, 2),
            make_entry("__torajs_str_lit_1", b"yo", true, 2),
        ];
        let layout = compute_user_strings_layout(&strings, 0x4000, 0x1_0000_4000);
        let mut sym_table = SymTable::new();
        apply_user_string_overrides(&layout, &mut sym_table);
        assert_eq!(sym_table.get("__torajs_str_lit_0"), Some(&0x1_0000_4000));
        assert_eq!(
            sym_table.get("__torajs_str_lit_1"),
            Some(&(0x1_0000_4000 + (STR_HEADER_SIZE as u64 + 2))),
        );
    }
}
