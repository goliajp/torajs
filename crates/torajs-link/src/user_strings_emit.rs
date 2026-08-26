//! SD-4c-prereq+e1 — emit pass for user-binary string literals.
//!
//! Materializes the packed `[u64 header, u32 length, u32 _pad,
//! [N x i8] bytes]` rodata Str layout per `UserStringEntry`,
//! producing one contiguous byte buffer ready to splice into
//! the `__TEXT,__cstring` region the user-strings layout pass
//! placed. See [`crate::user_strings_layout`] for the byte ABI
//! reference (LLVM-era `emit_static_str_global` layout).
//!
//! Also: `apply_user_string_overrides` registers each entry's
//! `sym → vaddr` into the effective sym table the linker's
//! `apply_relocs` pass consumes — codegen-emitted
//! `RelocKind::Page21 / PageOff12 { target_sym: e.sym }` ADRP+ADD
//! pairs resolve through this map.

use crate::exec::{UserStringEntry, UserStringKind};
use crate::resolve::SymTable;
use crate::user_strings_layout::{
    STATIC_LITERAL_FLAG, STR_FLAG_IS_LATIN1, STR_HEADER_SIZE, UserStringsLayout,
};

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
        // Leading pad up to this entry's slot (covers the first
        // entry's pre-base alignment + previous entry's trailing
        // 8-byte slot pad — see `compute_user_strings_layout` for
        // the dynobj-bucket-tagging alignment requirement).
        let target = (le.file_offset - layout.file_offset) as usize;
        while buf.len() < target {
            buf.push(0);
        }
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
        // Trailing pad up to the entry's slot end (per layout).
        let slot_end = target + le.payload_size as usize;
        while buf.len() < slot_end {
            buf.push(0);
        }
        debug_assert_eq!(
            buf.len() as u32 - (le.file_offset - layout.file_offset),
            le.payload_size,
            "emitted entry occupied bytes must match layout.payload_size"
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
        if let Some(alias) = &entry.payload_alias {
            sym_table.insert(alias.clone(), entry.vaddr + u64::from(STR_HEADER_SIZE));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user_strings_layout::compute_user_strings_layout;

    fn make_entry(sym: &str, bytes: &[u8], is_latin1: bool, length: u32) -> UserStringEntry {
        UserStringEntry {
            sym: sym.into(),
            bytes: bytes.to_vec(),
            is_latin1,
            length,
            kind: UserStringKind::StaticStr,
            payload_alias: None,
        }
    }

    fn make_raw_entry(sym: &str, bytes: &[u8]) -> UserStringEntry {
        UserStringEntry {
            sym: sym.into(),
            bytes: bytes.to_vec(),
            is_latin1: true,
            length: bytes.len() as u32,
            kind: UserStringKind::RawBytes,
            payload_alias: None,
        }
    }

    /// A literal's `__torajs_str_dyn_<i>` alias resolves to the
    /// payload behind its `__torajs_str_lit_<i>` header — one copy of
    /// the bytes, two names.
    #[test]
    fn payload_alias_registers_at_header_end() {
        let mut e = make_entry("__torajs_str_lit_0", b"hello", true, 5);
        e.payload_alias = Some("__torajs_str_dyn_0".into());
        let strings = [e];
        let layout = compute_user_strings_layout(&strings, 0x4100, 0x1_0000_4100);
        let mut table = SymTable::new();
        apply_user_string_overrides(&layout, &mut table);
        assert_eq!(table.get("__torajs_str_lit_0"), Some(&0x1_0000_4100));
        assert_eq!(
            table.get("__torajs_str_dyn_0"),
            Some(&(0x1_0000_4100 + u64::from(STR_HEADER_SIZE)))
        );
        let payload = build_user_strings_payload(&strings, &layout);
        assert_eq!(&payload[STR_HEADER_SIZE as usize..][..5], b"hello");
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

        // body = 16 + 2 = 18; slot = 24 (next 8-aligned for dynobj
        // key-ptr alignment). Two entries → total 48 bytes.
        let slot_size = 24usize;
        assert_eq!(buf.len(), slot_size * 2);
        // First entry payload @16 = "hi"; trailing 6 bytes zero pad.
        assert_eq!(&buf[16..18], b"hi");
        assert_eq!(&buf[18..slot_size], &[0u8; 6]);
        // Second entry payload @(slot_size + 16) = "yo".
        assert_eq!(&buf[slot_size + 16..slot_size + 18], b"yo");
    }

    #[test]
    fn raw_bytes_entry_emits_payload_only() {
        let strings = [make_raw_entry("__torajs_str_dyn_0", b"hello")];
        let layout = compute_user_strings_layout(&strings, 0x4000, 0x1_0000_4000);
        let buf = build_user_strings_payload(&strings, &layout);
        // body = 5; slot = 8 (next 8-aligned).
        assert_eq!(buf.len(), 8);
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(&buf[5..8], &[0u8; 3]);
    }

    #[test]
    fn mixed_kinds_concat_static_then_raw() {
        // Static (body=21 → slot=24) + RawBytes (body=3 → slot=8) = 32.
        let strings = [
            make_entry("__torajs_str_lit_0", b"hello", true, 5),
            make_raw_entry("__torajs_str_dyn_0", b"abc"),
        ];
        let layout = compute_user_strings_layout(&strings, 0x4000, 0x1_0000_4000);
        let buf = build_user_strings_payload(&strings, &layout);
        assert_eq!(buf.len(), 32);
        // Static entry: header + length + pad + "hello" @ [0..21].
        assert_eq!(&buf[16..21], b"hello");
        // Slot trailing pad @ [21..24].
        assert_eq!(&buf[21..24], &[0u8; 3]);
        // RawBytes entry starts at offset 24 — payload only.
        assert_eq!(&buf[24..27], b"abc");
        // RawBytes slot trailing pad @ [27..32].
        assert_eq!(&buf[27..32], &[0u8; 5]);
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
        // Entry 1 starts at entry 0's 8-aligned slot end (= 24).
        assert_eq!(sym_table.get("__torajs_str_lit_1"), Some(&0x1_0000_4018),);
    }
}
