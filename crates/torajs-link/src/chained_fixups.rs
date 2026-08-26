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

use crate::chained_fixups_starts::{RebaseTarget, build_starts_in_segment, round_up_4};

/// SD-4c-prereq+e7b-3 — `__TEXT` segment rebase descriptor.
///
/// Modern arm64 macOS binaries carry user-vtable slots inside
/// `__TEXT,__const` / `__cstring` rodata. Each slot is an 8-byte
/// chained-fixup link whose `target` field is the offset from
/// `text_vmaddr` to a `__torajs_fn_<fid>` body — dyld walks the
/// `__TEXT` chain at load time, adds the ASLR slide, and stamps
/// the runtime address into the slot. Pass `Some(...)` to
/// [`build_chained_fixups`] when the binary has at least one
/// vtable; pass `None` for syscall-abort / `_malloc` / `_str_alloc`
/// probes that ship no vtable at all.
#[derive(Debug, Clone)]
pub struct TextRebaseScope<'a> {
    /// Offset of `__TEXT` segment from the image base — equal to
    /// `0` in our PAGEZERO+TEXT+DATA+LINKEDIT layout because
    /// `__PAGEZERO` is unmapped and `__TEXT` is the first mapped
    /// segment.
    pub text_segment_vmaddr_offset: u64,
    /// `__TEXT` segment's vmsize (rounded up to whole pages by
    /// dyld), used to size the per-segment `page_start[]` table.
    pub text_segment_vmsize: u64,
    /// 0-based index of `__TEXT` inside the segment-ordered LC
    /// chain (= `1` in our PAGEZERO+TEXT+DATA+LINKEDIT order).
    pub text_seg_idx: u32,
    /// Per-vtable-slot rebase targets — `(offset_in_text_segment,
    /// target_offset_in_image)`. The matching encoded link values
    /// come back as `ChainedFixupsBlob.text_rebase_link_values` in
    /// the same order, so the emit pass can splice each one onto
    /// its slot byte position.
    pub rebase_targets: &'a [RebaseTarget],
}

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
/// `__LINKEDIT` as-is; each `*_values` vec is the 8-byte LE word
/// the emit pass splices onto its respective slot kind (la-ptr,
/// TLV thunk, vtable rebase, member-data rebase).
#[derive(Debug, Clone)]
pub struct ChainedFixupsBlob {
    pub blob: Vec<u8>,
    /// Per la-ptr import slot value, in `la_ptr_imports` BTreeMap iter order.
    pub la_ptr_slot_values: Vec<u64>,
    /// Per TLV thunk slot value, in `tlv_thunk_offsets` input order.
    pub tlv_thunk_link_values: Vec<u64>,
    /// e7b-3 vtable-slot rebase link values, indexed by
    /// `TextRebaseScope.rebase_targets[i]`. Empty when `text_rebase`
    /// was `None`.
    pub text_rebase_link_values: Vec<u64>,
    /// swap-2k chunk 2b — member-`__DATA,*` rebase link values,
    /// indexed by `data_rebase_targets[i]`. Empty when the slice
    /// was empty (the 2b-2/2b-3 inert path).
    pub data_rebase_link_values: Vec<u64>,
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

/// Encode one rebase chain link for `DYLD_CHAINED_PTR_64_OFFSET`
/// (format 6) — the format every modern arm64 macOS binary uses
/// for PIE pointer relocations inside `__DATA` and `__TEXT` rodata.
/// Follows `dyld_chained_ptr_64_rebase` in `<mach-o/fixup-chains.h>`:
///
/// ```text
///   target   : 36   bits 35..0   (offset from image base / text_vmaddr)
///   high8    :  8   bits 43..36  (high-byte tag — = 0 for regular ptrs)
///   reserved :  7   bits 50..44  (= 0)
///   next     : 12   bits 62..51  (stride in 4-byte units, 0 = chain end)
///   bind     :  1   bit 63       (= 0 for rebase)
/// ```
///
/// `target_offset_in_image` is the absolute offset from `text_vmaddr`
/// to the symbol the slot resolves to — dyld adds the actual ASLR
/// slide at load time. With 36 bits the encoding covers up to 64 GiB
/// of image space, which is well above any plausible torajs binary
/// (`tr` ships ≪ 64 MiB even at fat-LTO).
///
/// `next_stride` matches the bind encoder's semantics (4-byte units
/// to the next slot inside the same chained-fixups page, `0` ends
/// the chain). `high8` is reserved for tagged-pointer hints and stays
/// `0` for vtable / data-pointer slots.
pub fn encode_rebase_link(target_offset_in_image: u64, next_stride: u32, high8: u8) -> u64 {
    debug_assert!(
        target_offset_in_image < (1u64 << 36),
        "target_offset_in_image {target_offset_in_image} overflows 36-bit field"
    );
    debug_assert!(
        next_stride < (1 << 12),
        "next_stride {next_stride} overflows 12-bit field"
    );
    let target_field = target_offset_in_image & 0x0000_000F_FFFF_FFFF;
    let high8_field = (u64::from(high8) & 0xFF) << 36;
    let next_field = (u64::from(next_stride) & 0xFFF) << 51;
    // bind bit (63) stays 0 — that is what distinguishes a rebase
    // link from the bind link encode_bind_link emits.
    target_field | high8_field | next_field
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
/// `data_segment_filesize` is the `__DATA` segment's FILESIZE
/// (file-backed region only, NO zerofill), used to determine
/// `page_count` for the `dyld_chained_starts_in_segment.page_start[]`
/// table. Chunk 633: covering the zerofill pages made the kernel
/// page-in linker fault them in from the file (LINKEDIT bytes)
/// instead of zero-filling — see chained_fixups_starts.rs.
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
    data_segment_filesize: u64,
    tlv_thunk_offsets: &[u64],
    seg_count: u32,
    data_seg_idx: u32,
    text_rebase: Option<&TextRebaseScope<'_>>,
    data_rebase_targets: &[RebaseTarget],
) -> ChainedFixupsBlob {
    let has_text_rebase = text_rebase.is_some_and(|r| !r.rebase_targets.is_empty());
    let has_data_rebase = !data_rebase_targets.is_empty();
    debug_assert!(
        !la_ptr_imports.is_empty()
            || !tlv_thunk_offsets.is_empty()
            || has_text_rebase
            || has_data_rebase,
        "build_chained_fixups must be called with at least one chain \
         participant — empty case should short-circuit upstream"
    );
    debug_assert!(
        data_seg_idx < seg_count,
        "data_seg_idx {data_seg_idx} must be < seg_count {seg_count}"
    );
    // r505 — a `__DATA` segment with no file-backed byte (every
    // buffer zero-initialised, every table rodata) has no page a
    // chain could start on: it gets no `starts_in_segment` and its
    // `seg_info_offset` stays 0, exactly like a segment the chain
    // never touches. Any slot addressed into it would be a caller
    // bug — `build_starts_in_segment` asserts that.
    let data_has_pages = data_segment_filesize > 0;
    debug_assert!(
        data_has_pages
            || (la_ptr_imports.is_empty() && tlv_thunk_offsets.is_empty() && !has_data_rebase),
        "chain slots addressed into a __DATA segment with no file-backed pages"
    );
    if let Some(r) = text_rebase {
        debug_assert!(
            r.text_seg_idx < seg_count,
            "text_seg_idx {} must be < seg_count {seg_count}",
            r.text_seg_idx,
        );
        debug_assert!(
            r.text_seg_idx != data_seg_idx,
            "text_seg_idx {} must differ from data_seg_idx {data_seg_idx} — \
             __TEXT and __DATA cannot share a segment slot",
            r.text_seg_idx,
        );
        debug_assert!(
            r.text_segment_vmsize > 0,
            "text_segment_vmsize must be non-zero when rebase_targets is non-empty",
        );
    }

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

    // Build per-segment dyld_chained_starts_in_segment blocks:
    // - __DATA always carries la-ptr binds + TLV thunk binds.
    // - __TEXT only when text_rebase carries vtable rebase slots.
    //
    // Every chain participant gets bucketed into its segment page
    // and ordered by offset within the page. Walk each bucket to
    // emit one chain via `next_stride` links; chains never cross
    // page boundaries — each page starts its own chain via
    // page_start[page_idx].
    let data_starts = data_has_pages.then(|| {
        build_starts_in_segment(
            la_ptr_imports,
            la_ptr_offset_in_segment,
            data_segment_vmaddr_offset,
            data_segment_filesize,
            tlv_thunk_offsets,
            tlv_import_ordinal,
            data_rebase_targets,
        )
    });
    let data_starts_bytes_len = data_starts.as_ref().map_or(0, |s| s.bytes.len());
    let text_starts = text_rebase.map(|r| {
        build_starts_in_segment(
            &BTreeMap::new(),
            0,
            r.text_segment_vmaddr_offset,
            r.text_segment_vmsize,
            &[],
            0,
            r.rebase_targets,
        )
    });

    // Build dyld_chained_starts_in_image. Per-segment
    // seg_info_offset[] entries are `0` for segments with no
    // chains, or the byte offset (from the starts_in_image base)
    // of that segment's `dyld_chained_starts_in_segment` block.
    // __DATA always lives directly after the seg_info_offset[]
    // table; __TEXT (when present) follows __DATA.
    let starts_in_image_header_size: u32 = 4 + 4 * seg_count;
    let data_starts_off = starts_in_image_header_size;
    let text_starts_off = data_starts_off + data_starts_bytes_len as u32;
    let mut starts_in_image: Vec<u8> = Vec::with_capacity(starts_in_image_header_size as usize);
    starts_in_image.extend_from_slice(&seg_count.to_le_bytes());
    for seg in 0..seg_count {
        let off: u32 = if seg == data_seg_idx {
            if data_starts.is_some() {
                data_starts_off
            } else {
                0
            }
        } else if text_starts.is_some() && text_rebase.is_some_and(|r| seg == r.text_seg_idx) {
            text_starts_off
        } else {
            0
        };
        starts_in_image.extend_from_slice(&off.to_le_bytes());
    }

    // Concatenate: header + starts_in_image + per-seg starts → imports → symbols.
    let text_starts_bytes_len = text_starts.as_ref().map_or(0, |s| s.bytes.len());
    let starts_total_size = starts_in_image.len() + data_starts_bytes_len + text_starts_bytes_len;
    let imports_offset = FIXUPS_HEADER_SIZE + starts_total_size as u32;
    let imports_offset = round_up_4(imports_offset);
    let imports_count = imports_seq.len() as u32;
    let imports_blob_size = 4 * imports_count;
    let mut symbols_offset = imports_offset + imports_blob_size;
    symbols_offset = round_up_4(symbols_offset);

    let (imports_blob, sym_blob) = import_tables(&imports_seq);

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
    if let Some(ref ds) = data_starts {
        blob.extend_from_slice(&ds.bytes);
    }
    if let Some(ref ts) = text_starts {
        blob.extend_from_slice(&ts.bytes);
    }
    while blob.len() < imports_offset as usize {
        blob.push(0);
    }
    blob.extend_from_slice(&imports_blob);
    while blob.len() < symbols_offset as usize {
        blob.push(0);
    }
    blob.extend_from_slice(&sym_blob);

    let text_rebase_link_values = text_starts
        .map(|ts| ts.rebase_link_values)
        .unwrap_or_default();
    let (la_ptr_slot_values, tlv_thunk_link_values, data_rebase_link_values) = data_starts
        .map(|ds| {
            (
                ds.la_ptr_slot_values,
                ds.tlv_thunk_link_values,
                ds.rebase_link_values,
            )
        })
        .unwrap_or_default();
    ChainedFixupsBlob {
        blob,
        la_ptr_slot_values,
        tlv_thunk_link_values,
        text_rebase_link_values,
        data_rebase_link_values,
    }
}

/// The `dyld_chained_import[]` blob and the C string table it indexes,
/// for `imports_seq` in order: each import packs `(lib_ordinal,
/// weak_import = 0, name_offset)` — the ordinal picks which
/// `LC_LOAD_DYLIB` dyld binds against (1 = libSystem, 2 = libcurl) —
/// and the string table is 8-aligned so the codesign hash covers a
/// self-contained, padded blob.
fn import_tables(imports_seq: &[(&str, u8)]) -> (Vec<u8>, Vec<u8>) {
    let mut sym_blob: Vec<u8> = Vec::new();
    let mut name_offsets: Vec<(u32, u8)> = Vec::with_capacity(imports_seq.len());
    for &(name, lib_ordinal) in imports_seq {
        name_offsets.push((sym_blob.len() as u32, lib_ordinal));
        sym_blob.extend_from_slice(name.as_bytes());
        sym_blob.push(0);
    }
    while sym_blob.len() % 8 != 0 {
        sym_blob.push(0);
    }
    let mut imports_blob: Vec<u8> = Vec::with_capacity(4 * imports_seq.len());
    for &(name_off, lib_ordinal) in &name_offsets {
        let weak_import: u32 = 0;
        let packed: u32 = (u32::from(lib_ordinal) & 0xFF)
            | ((weak_import & 0x1) << 8)
            | ((name_off & 0x7F_FFFF) << 9);
        imports_blob.extend_from_slice(&packed.to_le_bytes());
    }
    (imports_blob, sym_blob)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_data_segment_without_file_pages_gets_no_starts_entry() {
        // __DATA_CONST carries one text rebase; __DATA has no file
        // byte (filesize 0) → its seg_info_offset is 0 and only the
        // text starts blob follows the image header.
        let targets = [(0x8u64, 0x1000u64)];
        let scope = TextRebaseScope {
            text_segment_vmaddr_offset: 0x4000,
            text_segment_vmsize: 0x4000,
            text_seg_idx: 2,
            rebase_targets: &targets,
        };
        let built =
            build_chained_fixups(&BTreeMap::new(), 0x8000, 0, 0, &[], 4, 3, Some(&scope), &[]);
        let b = &built.blob;
        let u = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        assert_eq!(u(32), 4);
        assert_eq!(u(36 + 4 * 3), 0, "__DATA: no starts");
        assert_eq!(
            u(36 + 4 * 2),
            4 + 4 * 4,
            "__DATA_CONST starts right after the image header"
        );
        assert!(built.la_ptr_slot_values.is_empty());
        assert_eq!(built.text_rebase_link_values.len(), 1);
    }

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
    fn encode_rebase_link_chain_end_keeps_bind_bit_clear() {
        // target offset 0x4000 (one page in), chain-end (next=0),
        // no high8. Bind bit (63) must stay 0 — that is the marker
        // dyld uses to distinguish rebase from bind.
        let v = encode_rebase_link(0x4000, 0, 0);
        assert_eq!(v, 0x4000);
        assert_eq!(v & (1u64 << 63), 0);
    }

    #[test]
    fn encode_rebase_link_next_stride_lands_in_top_bits() {
        // next stride 3 (= 12 bytes between linked slots).
        let v = encode_rebase_link(0, 3, 0);
        let expected = 3u64 << 51;
        assert_eq!(v, expected);
        assert_eq!(v & (1u64 << 63), 0);
    }

    #[test]
    fn encode_rebase_link_target_lands_in_low_36_bits() {
        // Target field covers bits 35..0; pick a 36-bit-full value to
        // confirm the mask and that high8 does not bleed through.
        let target: u64 = 0x0000_000F_FFFF_FFFF;
        let v = encode_rebase_link(target, 0, 0);
        assert_eq!(v & 0x0000_000F_FFFF_FFFF, target);
        // high8 field (bits 43..36) must be zero in the encoded word.
        assert_eq!((v >> 36) & 0xFF, 0);
    }

    #[test]
    fn encode_rebase_link_high8_lands_in_bits_43_36() {
        // high8 = 0xC0 — the typical hint dyld accepts for
        // tagged-pointer auth slots.
        let v = encode_rebase_link(0x1234, 0, 0xC0);
        assert_eq!(v & 0x0000_000F_FFFF_FFFF, 0x1234);
        assert_eq!((v >> 36) & 0xFF, 0xC0);
        assert_eq!(v & (1u64 << 63), 0);
    }

    #[test]
    fn encode_rebase_link_distinguishable_from_bind_link() {
        // Same numeric value (0) on both encoders — bind sets bit 63,
        // rebase does not. dyld dispatches the chain walk on this bit.
        let bind = encode_bind_link(0, 0, 0);
        let rebase = encode_rebase_link(0, 0, 0);
        assert_ne!(bind, rebase);
        assert_eq!(bind & (1u64 << 63), 1u64 << 63);
        assert_eq!(rebase & (1u64 << 63), 0);
    }

    #[test]
    fn single_import_blob_layout_invariants() {
        let imports = imports_of(&["_malloc"]);
        let result = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &[], 4, 2, None, &[]);
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
        let result = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &[], 4, 2, None, &[]);
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
        let result = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &[], 4, 2, None, &[]);
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
        let result = build_chained_fixups(
            &imports,
            data_vmaddr_offset,
            0,
            PAGE_SIZE,
            &[],
            4,
            2,
            None,
            &[],
        );
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
        let result = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &[], 4, 2, None, &[]);
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
        let result =
            build_chained_fixups(&imports, 0x8000, 0, 3 * PAGE_SIZE, &thunks, 4, 2, None, &[]);

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
        let result = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &thunks, 4, 2, None, &[]);

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
        let with_tlv =
            build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &[0x100], 4, 2, None, &[]);
        let without_tlv =
            build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &[], 4, 2, None, &[]);

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
        let result = build_chained_fixups(&imports, 0x8000, 0, PAGE_SIZE, &thunks, 4, 2, None, &[]);

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
