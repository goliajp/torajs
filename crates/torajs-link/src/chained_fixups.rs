//! `LC_DYLD_CHAINED_FIXUPS` blob encoder — SD-3a + SD-4c-prereq+c.
//!
//! Modern macOS (≥ 12 for arm64) uses `LC_DYLD_CHAINED_FIXUPS`
//! instead of the legacy `LC_DYLD_INFO_ONLY` opcode stream to
//! describe pointer relocations that dyld must apply at load
//! time. The blob lives inside `__LINKEDIT` and pairs with chain
//! values written directly into `__DATA,__la_symbol_ptr` (and
//! any other writable pointer-bearing section). SD-2/SD-3 added
//! the lazy pointer slot table dyld binds to libSystem entry
//! points referenced by stub trampolines; SD-4c-prereq+c extends
//! the same chain to cover every `__DATA,__thread_vars`
//! descriptor's `thunk` slot so dyld binds it to libSystem's
//! `__tlv_bootstrap` at startup — without this bind, the thunk
//! word stays zero and `_str_alloc` jumps through a null function
//! pointer the first time it reads a `thread_local!`.
//!
//! Wire format references:
//!
//! - `<mach-o/fixup-chains.h>` (Apple dyld source)
//! - `dyld3/MachOLoaded.cpp::ChainedFixupsBuilder` (reference impl)
//!
//! Top-level on-disk layout the blob carries:
//!
//! ```text
//!   [0..32]   dyld_chained_fixups_header
//!     0     fixups_version  = 0
//!     4     starts_offset   = 32           // dyld_chained_starts_in_image
//!     8     imports_offset                 // dyld_chained_import[]
//!    12     symbols_offset                 // C string table
//!    16     imports_count
//!    20     imports_format  = 1 (DYLD_CHAINED_IMPORT)
//!    24     symbols_format  = 0 (uncompressed)
//!    28     padding
//!
//!   [32..imports_offset]
//!     dyld_chained_starts_in_image
//!       seg_count           u32
//!       seg_info_offset[]   u32 × seg_count  // 0 = no fixups in segment
//!     dyld_chained_starts_in_segment (only for __DATA)
//!       size                u32
//!       page_size           u16  = 0x4000
//!       pointer_format      u16  = DYLD_CHAINED_PTR_64_OFFSET (6)
//!       segment_offset      u64  = __DATA vmaddr - image base
//!       max_valid_pointer   u32  = 0 for arm64
//!       page_count          u16
//!       page_start[]        u16 × page_count   (DYLD_CHAINED_PTR_START_NONE = 0xFFFF for empty)
//!
//!   [imports_offset..symbols_offset]
//!     dyld_chained_import[imports_count]
//!       lib_ordinal:8 | weak_import:1 | name_offset:23   (u32 LE per entry)
//!
//!   [symbols_offset..end]
//!     Symbol C string table, NUL-terminated.
//! ```
//!
//! Per-slot chain encoding (one 8-byte word in `__la_symbol_ptr`)
//! follows `dyld_chained_ptr_64_bind`:
//!
//! ```text
//!   ordinal   : 24    bits 23..0  (index into imports table)
//!   addend    :  8    bits 31..24
//!   reserved  : 19    bits 50..32 (= 0)
//!   next      : 12    bits 62..51 (stride in 4-byte units, 0 = chain end)
//!   bind      :  1    bit 63       (= 1 for bind)
//! ```
//!
//! The blob writer expects callers to short-circuit when there
//! are no dyld imports — empty input would still produce a valid
//! header but no segment, and downstream layout would emit a
//! useless LC chain.

use std::collections::BTreeMap;

use crate::chained_fixups_starts::{build_starts_in_segment, round_up_4};

/// `dyld_chained_fixups_header.imports_format` — one
/// `dyld_chained_import` per import, packed 32-bit entries.
pub const DYLD_CHAINED_IMPORT: u32 = 1;

/// `dyld_chained_starts_in_segment.pointer_format` — modern arm64
/// macOS default. Targets are offsets relative to the image base
/// (text_vmaddr). For pure-bind chains (our case) the encoding
/// matches `DYLD_CHAINED_PTR_64`; the format tag picks dyld's
/// chain-walking strategy.
pub const DYLD_CHAINED_PTR_64_OFFSET: u16 = 6;

/// `dyld_chained_starts_in_segment.page_start[]` sentinel for a
/// page with no chains. The single-page bind case writes the
/// real chain start (typically `0`) into `page_start[0]`.
pub const DYLD_CHAINED_PTR_START_NONE: u16 = 0xFFFF;

/// On-disk size of `dyld_chained_fixups_header`.
pub const FIXUPS_HEADER_SIZE: u32 = 32;

/// Apple Silicon page size — same constant the segment writer
/// uses (kept local so the chained-fixups module doesn't have to
/// import the `lc::APPLE_SILICON_PAGE_SIZE` symbol from the
/// load-command layer).
pub(crate) const PAGE_SIZE: u64 = 0x4000;

/// Symbol name for the libSystem TLV trampoline. Every TLV
/// descriptor's `thunk` slot binds to this entry point; dyld
/// patches the slot with the runtime address at process start.
pub const TLV_BOOTSTRAP_SYMBOL: &str = "__tlv_bootstrap";

/// `lib_ordinal` for libSystem — matches the `LC_LOAD_DYLIB`
/// emission order in `archive_emit::emit_binary` (libSystem is
/// always written first when `has_dyld`).
pub const LIBSYSTEM_LIB_ORDINAL: u8 = 1;

/// Result of building a chained-fixups blob. `blob` lands in
/// `__LINKEDIT` exactly as-is; `la_ptr_slot_values` is the
/// 8-byte sequence the emit pass writes into
/// `__DATA,__la_symbol_ptr` (one per import in
/// `la_ptr_imports` iter order); `tlv_thunk_link_values` is
/// the 8-byte sequence the emit pass overwrites onto each TLV
/// descriptor's `thunk` slot (one per entry in the input
/// `tlv_thunk_offsets`, same order).
#[derive(Debug, Clone)]
pub struct ChainedFixupsBlob {
    pub blob: Vec<u8>,
    /// Per la-ptr-import slot value (sorted by name, same order
    /// as the `BTreeMap` iter the caller passed in).
    pub la_ptr_slot_values: Vec<u64>,
    /// Per TLV thunk slot value, indexed by position in the
    /// `tlv_thunk_offsets` input. The emit pass pairs each value
    /// with the corresponding `TlvDescriptorLayout::thunk_slot_vaddr`
    /// and overwrites the 8-byte slot in the binary.
    pub tlv_thunk_link_values: Vec<u64>,
}

/// Encode one chain link for `__DATA,__la_symbol_ptr`. `ordinal`
/// is the 0-based index into the imports table. `next_stride` is
/// the offset (in 4-byte units) from this slot to the next link
/// in the chain — `0` terminates the chain. Returns the 8-byte
/// little-endian value that should occupy the slot.
pub fn encode_bind_link(ordinal: u32, next_stride: u32, addend: u8) -> u64 {
    debug_assert!(
        ordinal < (1 << 24),
        "ordinal {ordinal} overflows 24-bit field"
    );
    debug_assert!(
        next_stride < (1 << 12),
        "next_stride {next_stride} overflows 12-bit field"
    );
    let ordinal_field = u64::from(ordinal) & 0x00FF_FFFF;
    let addend_field = (u64::from(addend) & 0xFF) << 24;
    let next_field = (u64::from(next_stride) & 0xFFF) << 51;
    let bind_bit = 1u64 << 63;
    ordinal_field | addend_field | next_field | bind_bit
}

/// Build the complete chained-fixups blob plus the per-slot
/// chain link values for `__DATA,__la_symbol_ptr` and every TLV
/// descriptor's `thunk` slot.
///
/// `la_ptr_imports` is the `name → lib_ordinal` map for the
/// la-ptr-routed imports — `lib_ordinal` is packed into each
/// `dyld_chained_import.lib_ordinal` field so dyld picks the
/// right `LC_LOAD_DYLIB` per symbol. SD-4b's multi-dylib path
/// means the same blob can bind libSystem symbols (ordinal 1)
/// and libcurl symbols (ordinal 2) in one chain. The iter order
/// is sorted-by-name and matches the `__la_symbol_ptr` slot
/// layout the caller will write.
///
/// `data_segment_vmaddr_offset` is the offset of the `__DATA`
/// segment from the image base (`text_vmaddr`) — equal to
/// `text_vmsize` in our single-text-segment layout.
///
/// `la_ptr_offset_in_segment` is the byte offset of
/// `__la_symbol_ptr` from the start of `__DATA`. Our layout puts
/// it at offset `0`, but the parameter is kept explicit so the
/// blob format stays decoupled from `compute_archive_layout`'s
/// numeric choices.
///
/// `data_segment_vmsize` is the `__DATA` segment's vmsize (file
/// region + zerofill), used to determine `page_count` for the
/// `dyld_chained_starts_in_segment.page_start[]` table. The
/// total vmsize is rounded up to whole pages; pages with no
/// chained slot get the `DYLD_CHAINED_PTR_START_NONE` sentinel.
///
/// `tlv_thunk_offsets` is the list of `__DATA`-relative byte
/// offsets at which TLV descriptor `thunk` slots live. The
/// encoder appends `__tlv_bootstrap` to the imports table once
/// (lib_ordinal = libSystem) and emits one chain link per
/// offset pointing at that import. Pass an empty slice for
/// binaries that don't reference any thread-local variable
/// (syscall-abort / `_malloc` probes), in which case no
/// `__tlv_bootstrap` import is added.
///
/// `seg_count` is the total number of segments in the binary
/// (`PAGEZERO + TEXT + DATA + LINKEDIT = 4`). The
/// `dyld_chained_starts_in_image::seg_info_offset[]` array fills
/// `0` for every non-`__DATA` segment so dyld walks them as
/// no-chain.
///
/// `data_seg_idx` is the 0-based index of `__DATA` inside the
/// segment-ordered LC chain (= 2 in our PAGEZERO+TEXT+DATA+LINKEDIT
/// order).
pub fn build_chained_fixups(
    la_ptr_imports: &BTreeMap<String, u8>,
    data_segment_vmaddr_offset: u64,
    la_ptr_offset_in_segment: u64,
    data_segment_vmsize: u64,
    tlv_thunk_offsets: &[u64],
    seg_count: u32,
    data_seg_idx: u32,
) -> ChainedFixupsBlob {
    debug_assert!(
        !la_ptr_imports.is_empty() || !tlv_thunk_offsets.is_empty(),
        "build_chained_fixups must be called with at least one chain \
         participant — empty case should short-circuit upstream"
    );
    debug_assert!(
        data_seg_idx < seg_count,
        "data_seg_idx {data_seg_idx} must be < seg_count {seg_count}"
    );
    debug_assert!(
        data_segment_vmsize > 0,
        "data_segment_vmsize must be non-zero when chain participants exist"
    );

    // Layout the imports table in two consecutive ordinal ranges
    // so callers can map per-slot ordinals back without a name
    // lookup: la-ptr imports own ordinals `[0, la_ptr_n)` (sorted
    // by name via BTreeMap iter), then `__tlv_bootstrap` —
    // appended exactly once at the end — owns ordinal `la_ptr_n`.
    let la_ptr_n = la_ptr_imports.len();
    let has_tlv_chain = !tlv_thunk_offsets.is_empty();
    let tlv_import_ordinal: u32 = la_ptr_n as u32;

    // (name, lib_ordinal) pairs in final imports[] order.
    let mut imports_seq: Vec<(&str, u8)> =
        Vec::with_capacity(la_ptr_n + usize::from(has_tlv_chain));
    for (name, &ord) in la_ptr_imports {
        imports_seq.push((name.as_str(), ord));
    }
    if has_tlv_chain {
        imports_seq.push((TLV_BOOTSTRAP_SYMBOL, LIBSYSTEM_LIB_ORDINAL));
    }

    // Build the dyld_chained_starts_in_segment for __DATA with one
    // page_start[] entry per __DATA page. Every chain participant
    // (la-ptr slot + tlv thunk slot) is bucketed into its page
    // and ordered by offset within the page. Walk each bucket to
    // emit a single chain via `next_stride` links; chains never
    // cross page boundaries — each page starts its own chain via
    // page_start[page_idx].
    let starts_in_segment = build_starts_in_segment(
        la_ptr_imports,
        la_ptr_offset_in_segment,
        data_segment_vmaddr_offset,
        data_segment_vmsize,
        tlv_thunk_offsets,
        tlv_import_ordinal,
    );

    // Build dyld_chained_starts_in_image. Per-segment
    // seg_info_offset[] entries are `0` for segments with no
    // chains, or an offset into this very buffer (relative to
    // its start) for segments that do.
    let starts_in_image_header_size: u32 = 4 + 4 * seg_count;
    let mut starts_in_image: Vec<u8> = Vec::with_capacity(starts_in_image_header_size as usize);
    starts_in_image.extend_from_slice(&seg_count.to_le_bytes());
    for seg in 0..seg_count {
        let off: u32 = if seg == data_seg_idx {
            starts_in_image_header_size
        } else {
            0
        };
        starts_in_image.extend_from_slice(&off.to_le_bytes());
    }

    // Concatenate: header + starts_in_image + starts_in_segment
    // → imports table → symbol strings.
    let starts_total_size = starts_in_image.len() + starts_in_segment.bytes.len();
    let imports_offset = FIXUPS_HEADER_SIZE + starts_total_size as u32;
    let imports_offset = round_up_4(imports_offset);
    let imports_count = imports_seq.len() as u32;
    let imports_blob_size = 4 * imports_count;
    let mut symbols_offset = imports_offset + imports_blob_size;
    symbols_offset = round_up_4(symbols_offset);

    // Compose symbol strings table and remember each name's
    // starting offset + the dylib's lib_ordinal for the imports[]
    // entry build below.
    let mut sym_blob: Vec<u8> = Vec::new();
    let mut name_offsets: Vec<(u32, u8)> = Vec::with_capacity(imports_seq.len());
    for &(name, lib_ordinal) in &imports_seq {
        name_offsets.push((sym_blob.len() as u32, lib_ordinal));
        sym_blob.extend_from_slice(name.as_bytes());
        sym_blob.push(0);
    }
    // 8-byte align the tail so the codesign hash covers a
    // self-contained, padded blob.
    while sym_blob.len() % 8 != 0 {
        sym_blob.push(0);
    }

    // Assemble dyld_chained_import[] (4 bytes each, packed).
    // Per-import lib_ordinal picks which LC_LOAD_DYLIB dyld binds
    // against (1 = libSystem, 2 = libcurl, ...). weak_import = 0.
    let mut imports_blob: Vec<u8> = Vec::with_capacity(4 * imports_seq.len());
    for &(name_off, lib_ordinal) in &name_offsets {
        let weak_import: u32 = 0;
        let packed: u32 = (u32::from(lib_ordinal) & 0xFF)
            | ((weak_import & 0x1) << 8)
            | ((name_off & 0x7F_FFFF) << 9);
        imports_blob.extend_from_slice(&packed.to_le_bytes());
    }

    // Header.
    let mut blob: Vec<u8> = Vec::new();
    blob.extend_from_slice(&0u32.to_le_bytes()); // fixups_version
    blob.extend_from_slice(&FIXUPS_HEADER_SIZE.to_le_bytes()); // starts_offset
    blob.extend_from_slice(&imports_offset.to_le_bytes());
    blob.extend_from_slice(&symbols_offset.to_le_bytes());
    blob.extend_from_slice(&imports_count.to_le_bytes());
    blob.extend_from_slice(&DYLD_CHAINED_IMPORT.to_le_bytes()); // imports_format
    blob.extend_from_slice(&0u32.to_le_bytes()); // symbols_format = uncompressed
    blob.extend_from_slice(&0u32.to_le_bytes()); // header padding to 32 B
    debug_assert_eq!(blob.len() as u32, FIXUPS_HEADER_SIZE);

    blob.extend_from_slice(&starts_in_image);
    blob.extend_from_slice(&starts_in_segment.bytes);
    while blob.len() < imports_offset as usize {
        blob.push(0);
    }
    blob.extend_from_slice(&imports_blob);
    while blob.len() < symbols_offset as usize {
        blob.push(0);
    }
    blob.extend_from_slice(&sym_blob);

    ChainedFixupsBlob {
        blob,
        la_ptr_slot_values: starts_in_segment.la_ptr_slot_values,
        tlv_thunk_link_values: starts_in_segment.tlv_thunk_link_values,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper — all imports route through libSystem (ordinal 1).
    /// Tests that exercise multi-dylib ordinals use the direct
    /// `BTreeMap` literal instead.
    fn imports_of(names: &[&str]) -> BTreeMap<String, u8> {
        names.iter().map(|s| ((*s).to_string(), 1u8)).collect()
    }

    #[test]
    fn encode_bind_link_single_slot_chain_end() {
        // Single _malloc bind, last (only) slot in chain.
        let v = encode_bind_link(0, 0, 0);
        // bind bit set, all other fields zero.
        assert_eq!(v, 0x8000_0000_0000_0000);
    }

    #[test]
    fn encode_bind_link_next_stride_lands_in_top_bits() {
        let v = encode_bind_link(0, 2, 0);
        // next = 2 at bits 62..51 → 2 << 51.
        let expected = (1u64 << 63) | (2u64 << 51);
        assert_eq!(v, expected);
    }

    #[test]
    fn encode_bind_link_ordinal_lands_in_low_bits() {
        let v = encode_bind_link(0x42, 0, 0);
        // ordinal at bits 23..0, bind bit set.
        let expected = (1u64 << 63) | 0x42;
        assert_eq!(v, expected);
    }

    #[test]
    fn single_import_blob_layout_invariants() {
        let imports = imports_of(&["_malloc"]);
        let result = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &[], 4, 2);
        // Single slot, chain-end encoding.
        assert_eq!(result.la_ptr_slot_values.len(), 1);
        assert_eq!(result.la_ptr_slot_values[0], 0x8000_0000_0000_0000);
        // No TLV thunks → empty tlv link vec.
        assert!(result.tlv_thunk_link_values.is_empty());

        // Header sanity.
        let header = &result.blob[0..32];
        assert_eq!(u32::from_le_bytes(header[0..4].try_into().unwrap()), 0);
        assert_eq!(
            u32::from_le_bytes(header[4..8].try_into().unwrap()),
            FIXUPS_HEADER_SIZE
        );
        let imports_offset = u32::from_le_bytes(header[8..12].try_into().unwrap());
        let symbols_offset = u32::from_le_bytes(header[12..16].try_into().unwrap());
        let imports_count = u32::from_le_bytes(header[16..20].try_into().unwrap());
        let imports_format = u32::from_le_bytes(header[20..24].try_into().unwrap());
        let symbols_format = u32::from_le_bytes(header[24..28].try_into().unwrap());
        assert_eq!(imports_count, 1);
        assert_eq!(imports_format, DYLD_CHAINED_IMPORT);
        assert_eq!(symbols_format, 0);

        // imports table at imports_offset must be exactly 4 bytes.
        let import_word = u32::from_le_bytes(
            result.blob[imports_offset as usize..imports_offset as usize + 4]
                .try_into()
                .unwrap(),
        );
        // lib_ordinal=1 in low 8 bits, weak_import=0, name_offset=0.
        assert_eq!(import_word & 0xFF, 1);
        assert_eq!((import_word >> 8) & 0x1, 0);
        assert_eq!((import_word >> 9) & 0x7F_FFFF, 0);

        // Symbol string at symbols_offset must be "_malloc\0".
        let name_bytes = &result.blob[symbols_offset as usize..symbols_offset as usize + 8];
        assert_eq!(&name_bytes[..7], b"_malloc");
        assert_eq!(name_bytes[7], 0);
    }

    #[test]
    fn multi_import_slot_values_chain_through_then_terminate() {
        let imports = imports_of(&["_malloc", "_free", "_pthread_create"]);
        let result = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &[], 4, 2);
        // BTreeSet iter order = sorted: _free, _malloc, _pthread_create
        assert_eq!(result.la_ptr_slot_values.len(), 3);
        // First two slots have next = 2 (8 bytes ahead at stride 4).
        let next_of = |v: u64| (v >> 51) & 0xFFF;
        let ordinal_of = |v: u64| v & 0xFF_FFFF;
        assert_eq!(next_of(result.la_ptr_slot_values[0]), 2);
        assert_eq!(next_of(result.la_ptr_slot_values[1]), 2);
        // Last slot terminates the chain.
        assert_eq!(next_of(result.la_ptr_slot_values[2]), 0);
        // Ordinals march 0, 1, 2 in sorted-by-name order.
        assert_eq!(ordinal_of(result.la_ptr_slot_values[0]), 0);
        assert_eq!(ordinal_of(result.la_ptr_slot_values[1]), 1);
        assert_eq!(ordinal_of(result.la_ptr_slot_values[2]), 2);
    }

    #[test]
    fn starts_in_image_seg_info_offset_lights_data_segment() {
        let imports = imports_of(&["_malloc"]);
        // seg_count = 4 (PAGEZERO + TEXT + DATA + LINKEDIT),
        // __DATA is at index 2.
        let result = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &[], 4, 2);
        // dyld_chained_starts_in_image starts at byte 32 (after header).
        let starts_off = FIXUPS_HEADER_SIZE as usize;
        let seg_count =
            u32::from_le_bytes(result.blob[starts_off..starts_off + 4].try_into().unwrap());
        assert_eq!(seg_count, 4);
        // seg_info_offset[0..3] — only index 2 (DATA) is non-zero.
        for seg in 0..4u32 {
            let off_pos = starts_off + 4 + (seg as usize) * 4;
            let off = u32::from_le_bytes(result.blob[off_pos..off_pos + 4].try_into().unwrap());
            if seg == 2 {
                // Must point at the dyld_chained_starts_in_segment
                // for __DATA, which sits directly after the
                // dyld_chained_starts_in_image header in this blob.
                assert!(off > 0, "DATA seg_info_offset must be non-zero");
            } else {
                assert_eq!(off, 0, "non-DATA segment {seg} must have no chains");
            }
        }
    }

    #[test]
    fn starts_in_segment_fields_for_arm64_macos_layout() {
        let imports = imports_of(&["_malloc"]);
        let data_vmaddr_offset = 0x8000u64;
        let result = build_chained_fixups(&imports, data_vmaddr_offset, 0, PAGE_SIZE, &[], 4, 2);
        // Locate dyld_chained_starts_in_segment via seg_info_offset[2].
        let starts_off = FIXUPS_HEADER_SIZE as usize;
        let off_pos = starts_off + 4 + 2 * 4;
        let seg_info_off =
            u32::from_le_bytes(result.blob[off_pos..off_pos + 4].try_into().unwrap()) as usize;
        let base = starts_off + seg_info_off;
        let page_size = u16::from_le_bytes(result.blob[base + 4..base + 6].try_into().unwrap());
        let pointer_format =
            u16::from_le_bytes(result.blob[base + 6..base + 8].try_into().unwrap());
        let segment_offset =
            u64::from_le_bytes(result.blob[base + 8..base + 16].try_into().unwrap());
        let max_valid = u32::from_le_bytes(result.blob[base + 16..base + 20].try_into().unwrap());
        let page_count = u16::from_le_bytes(result.blob[base + 20..base + 22].try_into().unwrap());
        let page_start = u16::from_le_bytes(result.blob[base + 22..base + 24].try_into().unwrap());

        assert_eq!(page_size, PAGE_SIZE as u16);
        assert_eq!(pointer_format, DYLD_CHAINED_PTR_64_OFFSET);
        assert_eq!(segment_offset, data_vmaddr_offset);
        assert_eq!(max_valid, 0);
        assert_eq!(page_count, 1);
        assert_eq!(page_start, 0); // first chain at byte 0 of the page
    }

    /// Round-up helper covers the explicit padding the blob
    /// inserts before imports / symbols. Without it the imports
    /// table could land 1-3 bytes into the previous structure
    /// because `dyld_chained_starts_in_segment` ends on a 2-byte
    /// boundary in the single-page-count case.
    #[test]
    fn imports_and_symbols_offsets_are_4_aligned() {
        let imports = imports_of(&["_a", "_bb", "_ccc"]);
        let result = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &[], 4, 2);
        let header = &result.blob[0..32];
        let imports_offset = u32::from_le_bytes(header[8..12].try_into().unwrap());
        let symbols_offset = u32::from_le_bytes(header[12..16].try_into().unwrap());
        assert_eq!(imports_offset % 4, 0);
        assert_eq!(symbols_offset % 4, 0);
    }

    /// Decoder helpers — slice the dyld_chained_starts_in_segment
    /// blob out of `result.blob` and pull page_count / page_start[]
    /// without each test having to re-walk the header pointers.
    fn starts_in_segment_base(result: &ChainedFixupsBlob) -> usize {
        let starts_off = FIXUPS_HEADER_SIZE as usize;
        let off_pos = starts_off + 4 + 2 * 4;
        let seg_info_off =
            u32::from_le_bytes(result.blob[off_pos..off_pos + 4].try_into().unwrap()) as usize;
        starts_off + seg_info_off
    }

    fn read_page_starts(result: &ChainedFixupsBlob) -> Vec<u16> {
        let base = starts_in_segment_base(result);
        let page_count =
            u16::from_le_bytes(result.blob[base + 20..base + 22].try_into().unwrap()) as usize;
        let mut out = Vec::with_capacity(page_count);
        for i in 0..page_count {
            let off = base + 22 + i * 2;
            out.push(u16::from_le_bytes(
                result.blob[off..off + 2].try_into().unwrap(),
            ));
        }
        out
    }

    /// SD-4c-prereq+c — TLV thunk bind in a __DATA whose vmsize
    /// spans multiple pages. la_ptr lives on page 0; two thunk
    /// slots sit on page 0 (chained off la_ptr) and page 2
    /// (independent chain start).
    #[test]
    fn tlv_thunks_across_multiple_pages_get_independent_chain_starts() {
        let imports = imports_of(&["_malloc"]);
        // __DATA spans 3 pages (48 KB). la_ptr at offset 0 (page 0).
        // Two thunks: one in page 0 (offset 0x100), one in page 2
        // (offset 0x8010).
        let thunks = vec![0x100u64, 0x8010u64];
        let result = build_chained_fixups(&imports, 0x8000, 0, 3 * PAGE_SIZE, &thunks, 4, 2);

        // imports[] = [_malloc (ord 0), __tlv_bootstrap (ord 1)].
        let header = &result.blob[0..32];
        let imports_count = u32::from_le_bytes(header[16..20].try_into().unwrap());
        assert_eq!(imports_count, 2);

        // page_count = 3, page_start[] reports a chain on every
        // page that hosts a slot; page 1 is the empty sentinel.
        let page_starts = read_page_starts(&result);
        assert_eq!(page_starts.len(), 3);
        assert_eq!(page_starts[0], 0, "la_ptr chain head at page 0 offset 0");
        assert_eq!(
            page_starts[1], DYLD_CHAINED_PTR_START_NONE,
            "no chain on page 1"
        );
        assert_eq!(
            page_starts[2], 0x10,
            "page 2 thunk owns its own chain start"
        );

        // Page 0 chain: la_ptr (offset 0) → thunk (offset 0x100).
        // next_stride = 0x100 / 4 = 0x40 = 64.
        let next_of = |v: u64| (v >> 51) & 0xFFF;
        let ordinal_of = |v: u64| v & 0xFF_FFFF;
        assert_eq!(next_of(result.la_ptr_slot_values[0]), 64);
        assert_eq!(ordinal_of(result.la_ptr_slot_values[0]), 0);
        // Page 0's second slot (thunk at 0x100) terminates the
        // page-0 chain.
        assert_eq!(next_of(result.tlv_thunk_link_values[0]), 0);
        assert_eq!(ordinal_of(result.tlv_thunk_link_values[0]), 1);
        // Page 2 thunk is a solo chain that terminates.
        assert_eq!(next_of(result.tlv_thunk_link_values[1]), 0);
        assert_eq!(ordinal_of(result.tlv_thunk_link_values[1]), 1);
    }

    /// Two thunks on the same page must chain via next_stride
    /// computed from their actual offset delta — not the la_ptr
    /// hard-coded stride=2. Catches regressions where the
    /// per-page link math falls back to the old single-page
    /// constant.
    #[test]
    fn tlv_thunks_same_page_chain_via_offset_delta() {
        let imports = BTreeMap::new();
        // 4 thunks all on page 0, at offsets 0x40, 0x68, 0x80, 0xC0.
        // Deltas in bytes: 0x28, 0x18, 0x40 → strides in 4-byte
        // units: 0xA, 0x6, 0x10.
        let thunks = vec![0x40u64, 0x68, 0x80, 0xC0];
        let result = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &thunks, 4, 2);

        let next_of = |v: u64| (v >> 51) & 0xFFF;
        let page_starts = read_page_starts(&result);
        assert_eq!(page_starts.len(), 1);
        assert_eq!(page_starts[0], 0x40);
        assert_eq!(next_of(result.tlv_thunk_link_values[0]), 0xA);
        assert_eq!(next_of(result.tlv_thunk_link_values[1]), 0x6);
        assert_eq!(next_of(result.tlv_thunk_link_values[2]), 0x10);
        assert_eq!(next_of(result.tlv_thunk_link_values[3]), 0);
        // imports[] is just __tlv_bootstrap → ordinal 0.
        let header = &result.blob[0..32];
        let imports_count = u32::from_le_bytes(header[16..20].try_into().unwrap());
        assert_eq!(imports_count, 1);
        let ordinal_of = |v: u64| v & 0xFF_FFFF;
        for v in &result.tlv_thunk_link_values {
            assert_eq!(ordinal_of(*v), 0);
        }
    }

    /// __tlv_bootstrap appears in the symbol string table when
    /// (and only when) tlv_thunk_offsets is non-empty, so the
    /// imports table layout the SD-3 chain decoder expects holds.
    #[test]
    fn tlv_bootstrap_string_appears_exactly_when_tlv_present() {
        let imports = imports_of(&["_malloc"]);
        let with_tlv = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &[0x100], 4, 2);
        let without_tlv = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &[], 4, 2);

        let contains_tlv = |blob: &[u8]| {
            blob.windows(TLV_BOOTSTRAP_SYMBOL.len())
                .any(|w| w == TLV_BOOTSTRAP_SYMBOL.as_bytes())
        };
        assert!(contains_tlv(&with_tlv.blob));
        assert!(!contains_tlv(&without_tlv.blob));
    }

    /// la_ptr_imports = empty, tlv_thunk_offsets = single slot.
    /// Tests the pure-TLV path (no SD-3 la-ptr imports) — every
    /// chain field comes from the tlv branch.
    #[test]
    fn pure_tlv_chain_with_no_la_ptr_imports() {
        let imports: BTreeMap<String, u8> = BTreeMap::new();
        let thunks = vec![0x200u64];
        let result = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &thunks, 4, 2);

        let header = &result.blob[0..32];
        let imports_count = u32::from_le_bytes(header[16..20].try_into().unwrap());
        assert_eq!(imports_count, 1, "only __tlv_bootstrap in imports[]");

        // la_ptr output is empty, tlv output has one entry.
        assert!(result.la_ptr_slot_values.is_empty());
        assert_eq!(result.tlv_thunk_link_values.len(), 1);
        let v = result.tlv_thunk_link_values[0];
        // Chain end, ordinal 0 (= __tlv_bootstrap, the only entry).
        let next_of = (v >> 51) & 0xFFF;
        let ordinal_of = v & 0xFF_FFFF;
        assert_eq!(next_of, 0);
        assert_eq!(ordinal_of, 0);

        // page_count = 1, page_start[0] = 0x200 (the thunk offset).
        let page_starts = read_page_starts(&result);
        assert_eq!(page_starts, vec![0x200]);
    }
}
