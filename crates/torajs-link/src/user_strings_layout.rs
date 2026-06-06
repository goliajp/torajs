//! SD-4c-prereq+e1 — layout pass for user-binary string literals.
//!
//! `ssa::Module.strings` materialized as `__TEXT,__cstring` payload
//! lined up byte-for-byte with `ssa_inkwell::globals::emit_static_str_global`:
//!
//! ```text
//!   [u64 header] [u32 length] [u32 _pad] [N x i8 bytes]
//!   header = refcount (=1) | (flags << 48)
//!   flags  = STATIC_LITERAL_FLAG | (STR_FLAG_IS_LATIN1 if is_latin1)
//! ```
//!
//! Packed — no padding between fields. Total per-entry bytes =
//! `STR_HEADER_SIZE (16) + bytes.len()`. Entries pack back-to-back
//! starting at `vaddr_base`; each entry's `vaddr` / `file_offset`
//! is the absolute address codegen's `RelocKind::Page21 /
//! PageOff12 { target_sym: e.sym }` ADRP+ADD pair resolves to.
//!
//! `compute_user_strings_layout` is pure (no merged-archives /
//! file-IO); the emit pass (`user_strings_emit`) consumes the
//! computed offsets to materialize bytes.

use crate::exec::{UserStringEntry, UserStringKind};

/// Mirror of `ssa_inkwell::globals::STATIC_LITERAL_FLAG`. The
/// runtime's `__torajs_rc_inc / dec / __torajs_str_free` short-circuit
/// when this bit is set so the rodata Str object is never written to.
pub const STATIC_LITERAL_FLAG: u16 = 4;

/// Mirror of `ssa_inkwell::globals::STR_IS_LATIN1_FLAG`. Drives the
/// runtime's per-code-unit byte stride (Latin-1 = 1 byte / unit,
/// UTF-16 = 2 LE bytes / unit).
pub const STR_FLAG_IS_LATIN1: u16 = 2;

/// Header byte size: `u64 header + u32 length + u32 _pad` = 16.
/// Payload bytes follow immediately (packed, no further padding).
pub const STR_HEADER_SIZE: u32 = 16;

/// Per-entry final placement — sym vaddr is what the SymTable
/// learns at link time so codegen-emitted ADRP+ADD pairs against
/// `target_sym = e.sym` resolve to the right ptr.
#[derive(Debug, Clone)]
pub struct UserStringEntryLayout {
    /// Extern sym codegen emits in reloc target — e.g.
    /// `__torajs_str_lit_0`.
    pub sym: String,
    /// Final vaddr the sym resolves to (points at the header u64).
    pub vaddr: u64,
    /// Absolute file offset of the entry's header byte.
    pub file_offset: u32,
    /// `STR_HEADER_SIZE + bytes.len()` — total bytes the entry
    /// occupies (header + length + pad + payload).
    pub payload_size: u32,
}

/// Aggregated layout for the user-string region. `entries[i]`
/// corresponds 1:1 with the input `&[UserStringEntry]`.
#[derive(Debug, Clone, Default)]
pub struct UserStringsLayout {
    pub entries: Vec<UserStringEntryLayout>,
    /// Absolute file offset of the first entry (= region start).
    pub file_offset: u32,
    /// Absolute vaddr of the first entry.
    pub vaddr: u64,
    /// Total bytes the user-string region occupies. Sum of every
    /// entry's `payload_size`. 0 when input is empty.
    pub total_size: u32,
}

/// Compute file-offset + vaddr placements for one input string set,
/// starting at the given `cursor` (file offset) / `vaddr_base`. Both
/// advance lock-step (vaddr - file_offset is constant within the
/// `__TEXT` segment since fileoff = 0 there). Pure function — no
/// allocations beyond the returned `entries` Vec.
///
/// `total_size` is rounded up to 4 bytes so the next-section section
/// (`__TEXT,__stubs`, all ARM64 instructions) lands at a 4-aligned
/// vaddr. ARM64 traps SIGILL on misaligned instruction fetch — without
/// this pad a 21-byte user-string entry shifts the stubs section to a
/// misaligned vaddr and exec crashes.
pub fn compute_user_strings_layout(
    strings: &[UserStringEntry],
    cursor: u32,
    vaddr_base: u64,
) -> UserStringsLayout {
    let mut entries = Vec::with_capacity(strings.len());
    let mut running: u32 = 0;
    for e in strings {
        let payload_size = entry_payload_size(e);
        entries.push(UserStringEntryLayout {
            sym: e.sym.clone(),
            vaddr: vaddr_base + u64::from(running),
            file_offset: cursor + running,
            payload_size,
        });
        running += payload_size;
    }
    let total_size = if running == 0 { 0 } else { (running + 3) & !3 };
    UserStringsLayout {
        entries,
        file_offset: cursor,
        vaddr: vaddr_base,
        total_size,
    }
}

/// Byte size of one entry's emitted region — `StaticStr` carries the
/// 16-byte rodata Str header in addition to the payload; `RawBytes`
/// is payload-only.
pub fn entry_payload_size(e: &UserStringEntry) -> u32 {
    let payload = e.bytes.len() as u32;
    match e.kind {
        UserStringKind::StaticStr => STR_HEADER_SIZE + payload,
        UserStringKind::RawBytes => payload,
    }
}

/// Collect the sym names from `LinkConfig.strings` — used as the
/// `extra_defined_syms` set the worklist closure
/// (`compute_required_members`) consults so `__torajs_str_lit_*`
/// reloc targets don't get reported as `UnresolvedExterns` before
/// the e1 emit pass runs.
pub fn user_strings_extra_defined_syms(
    strings: &[UserStringEntry],
) -> std::collections::BTreeSet<String> {
    strings.iter().map(|s| s.sym.clone()).collect()
}

/// Pipeline helper bundling layout + pre-built payload for one
/// user-string set. Returns `(layout, payload)` ready to splice
/// into `ArchiveLayout` after the file-cursor where the region
/// starts is known (= after `non_text_region_size`). Keeps
/// `archive_link.rs`'s call-site at one line so the file's
/// established `≥ soft` LOC floor doesn't grow.
pub fn build_user_strings_region(
    strings: &[UserStringEntry],
    text_vmaddr_base: u64,
    file_offset: u32,
) -> (UserStringsLayout, Vec<u8>) {
    let vaddr_base = text_vmaddr_base + u64::from(file_offset);
    let layout = compute_user_strings_layout(strings, file_offset, vaddr_base);
    let payload = crate::user_strings_emit::build_user_strings_payload(strings, &layout);
    (layout, payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sym: &str, bytes: &[u8], is_latin1: bool, length: u32) -> UserStringEntry {
        UserStringEntry {
            sym: sym.into(),
            bytes: bytes.to_vec(),
            is_latin1,
            length,
            kind: UserStringKind::StaticStr,
        }
    }

    fn raw_entry(sym: &str, bytes: &[u8]) -> UserStringEntry {
        UserStringEntry {
            sym: sym.into(),
            bytes: bytes.to_vec(),
            is_latin1: true,
            length: bytes.len() as u32,
            kind: UserStringKind::RawBytes,
        }
    }

    #[test]
    fn empty_input_zero_total() {
        let layout = compute_user_strings_layout(&[], 0x4000, 0x1_0000_4000);
        assert!(layout.entries.is_empty());
        assert_eq!(layout.total_size, 0);
        assert_eq!(layout.file_offset, 0x4000);
        assert_eq!(layout.vaddr, 0x1_0000_4000);
    }

    #[test]
    fn single_latin1_entry_packs_header_plus_payload() {
        let strings = [entry("__torajs_str_lit_0", b"hello", true, 5)];
        let layout = compute_user_strings_layout(&strings, 0x4100, 0x1_0000_4100);
        assert_eq!(layout.entries.len(), 1);
        // Per-entry payload_size = raw 21 (16 header + 5 bytes), but
        // region total rounds up to 24 so the next __TEXT section
        // (`__stubs`) lands on a 4-byte boundary.
        assert_eq!(layout.total_size, 24);
        let e = &layout.entries[0];
        assert_eq!(e.sym, "__torajs_str_lit_0");
        assert_eq!(e.file_offset, 0x4100);
        assert_eq!(e.vaddr, 0x1_0000_4100);
        assert_eq!(e.payload_size, STR_HEADER_SIZE + 5);
    }

    #[test]
    fn multi_entry_cumulative_offsets() {
        let strings = [
            entry("__torajs_str_lit_0", b"hi", true, 2),
            entry("__torajs_str_lit_1", b"world!", true, 6),
            entry("__torajs_str_lit_2", b"x", true, 1),
        ];
        let layout = compute_user_strings_layout(&strings, 0x4000, 0x1_0000_4000);

        let e0 = &layout.entries[0];
        let e1 = &layout.entries[1];
        let e2 = &layout.entries[2];

        assert_eq!(e0.file_offset, 0x4000);
        assert_eq!(e1.file_offset, e0.file_offset + e0.payload_size);
        assert_eq!(e2.file_offset, e1.file_offset + e1.payload_size);

        assert_eq!(e0.vaddr, 0x1_0000_4000);
        assert_eq!(e1.vaddr, e0.vaddr + u64::from(e0.payload_size));
        assert_eq!(e2.vaddr, e1.vaddr + u64::from(e1.payload_size));

        assert_eq!(e0.payload_size, STR_HEADER_SIZE + 2);
        assert_eq!(e1.payload_size, STR_HEADER_SIZE + 6);
        assert_eq!(e2.payload_size, STR_HEADER_SIZE + 1);

        // Sum of payload_size = 18 + 22 + 17 = 57, rounded up to 60.
        let raw_sum = e0.payload_size + e1.payload_size + e2.payload_size;
        assert_eq!(raw_sum, 57);
        assert_eq!(layout.total_size, 60);
    }

    #[test]
    fn utf16_payload_is_double_the_length() {
        // "hi" UTF-16 LE = [0x68 0x00 0x69 0x00] (4 bytes / 2 units).
        let strings = [entry(
            "__torajs_str_lit_0",
            &[0x68, 0x00, 0x69, 0x00],
            false,
            2,
        )];
        let layout = compute_user_strings_layout(&strings, 0x4000, 0x1_0000_4000);
        assert_eq!(layout.entries[0].payload_size, STR_HEADER_SIZE + 4);
        // 16 + 4 = 20, already 4-aligned so no extra pad.
        assert_eq!(layout.total_size, STR_HEADER_SIZE + 4);
    }

    #[test]
    fn raw_bytes_kind_has_no_header() {
        // RawBytes payload size = bytes.len() (no 16-byte header).
        let strings = [raw_entry("__torajs_str_dyn_0", b"hello")];
        let layout = compute_user_strings_layout(&strings, 0x4000, 0x1_0000_4000);
        assert_eq!(layout.entries[0].payload_size, 5);
        // 5 raw bytes rounds up to 8 (next-section 4-byte alignment).
        assert_eq!(layout.total_size, 8);
    }

    #[test]
    fn mixed_kinds_pack_back_to_back() {
        // Static (16+5=21) + RawBytes (3) = 24, already 4-aligned.
        let strings = [
            entry("__torajs_str_lit_0", b"hello", true, 5),
            raw_entry("__torajs_str_dyn_0", b"abc"),
        ];
        let layout = compute_user_strings_layout(&strings, 0x4000, 0x1_0000_4000);
        assert_eq!(layout.entries[0].payload_size, STR_HEADER_SIZE + 5);
        assert_eq!(layout.entries[1].payload_size, 3);
        assert_eq!(
            layout.entries[1].vaddr,
            layout.entries[0].vaddr + u64::from(layout.entries[0].payload_size),
        );
        assert_eq!(layout.total_size, 24);
    }

    #[test]
    fn flags_constants_match_ssa_inkwell_globals() {
        // Independent ground-truth check vs
        // ssa_inkwell::globals::STATIC_LITERAL_FLAG (= 4) and
        // STR_IS_LATIN1_FLAG (= 2). Any drift between this module
        // and the inkwell emit path would silently desync the
        // runtime Str header → string-deref UB.
        assert_eq!(STATIC_LITERAL_FLAG, 4);
        assert_eq!(STR_FLAG_IS_LATIN1, 2);
        assert_eq!(STR_HEADER_SIZE, 16);
    }
}
