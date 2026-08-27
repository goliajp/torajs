//! Per-`__DATA`-page chain bucketing for `LC_DYLD_CHAINED_FIXUPS` —
//! extracted from [`crate::chained_fixups`] so the top-level blob
//! encoder stays under the function-size limit while the per-page
//! chain math gets its own module-scoped namespace.
//!
//! See [`crate::chained_fixups`] for the on-disk wire format and the
//! per-slot bind-link encoding ([`crate::chained_fixups::encode_bind_link`]).

use std::collections::BTreeMap;

use crate::chained_fixups::{
    DYLD_CHAINED_PTR_64_OFFSET, DYLD_CHAINED_PTR_START_NONE, PAGE_SIZE, encode_bind_link,
    encode_rebase_link,
};

/// Per-rebase-slot input — `(offset_in_segment, target_offset_in_image)`.
/// `offset_in_segment` locates the 8-byte slot inside the chained
/// segment (so dyld can compute the page bucket); `target_offset_in_image`
/// is the offset from `text_vmaddr` to the symbol the slot resolves
/// to and lands directly in the format-6 `target` field. Used for
/// the `__DATA_CONST` tables' absolute slots (class_layouts / name
/// tables / baked regex) and member `__DATA,*` rebases.
pub(crate) type RebaseTarget = (u64, u64);

/// Pre-encoded `dyld_chained_starts_in_segment` blob together
/// with the per-slot 8-byte chain link values the emit pass
/// writes into `__DATA,__la_symbol_ptr`, each TLV descriptor's
/// `thunk` slot, and each rebase slot (`__DATA_CONST` / data rebase).
/// Split out so [`crate::chained_fixups::build_chained_fixups`]
/// stays under the function-size limit while the per-page chain math
/// gets dedicated coverage.
pub(crate) struct StartsInSegment {
    pub(crate) bytes: Vec<u8>,
    pub(crate) la_ptr_slot_values: Vec<u64>,
    pub(crate) tlv_thunk_link_values: Vec<u64>,
    /// Per-rebase-target encoded chain-link value, indexed by the
    /// input `rebase_targets` slice order. Empty when no rebase
    /// slot participates in this segment's chain.
    pub(crate) rebase_link_values: Vec<u64>,
}

/// Encode the `dyld_chained_starts_in_segment` blob for one writable
/// segment, bucketing every chain participant (la-ptr, TLV thunk,
/// rebase slot) into its segment page and linking each bucket's
/// slots via `next_stride`. The per-slot chain link values are
/// returned alongside so the emit pass can write them into the
/// binary at the matching offsets.
///
/// `la_ptr_imports` + `tlv_thunk_offsets` are the SD-3 bind-chain
/// participants and stay parameterised so __DATA-only callers keep
/// working unchanged. `rebase_targets` is the SD-4c-prereq+e7b
/// addition — pass an empty slice for segments with no rebase slot
/// (every existing caller does, until e7b-3 wires vtable rebase
/// into the __TEXT chain).
pub(crate) fn build_starts_in_segment(
    la_ptr_imports: &BTreeMap<String, u8>,
    la_ptr_offset_in_segment: u64,
    data_segment_vmaddr_offset: u64,
    segment_file_size: u64,
    tlv_thunk_offsets: &[u64],
    tlv_import_ordinal: u32,
    rebase_targets: &[RebaseTarget],
) -> StartsInSegment {
    /// Identifies which output vec a slot's encoded chain link
    /// belongs to so callers can match it back to the input
    /// `la_ptr_imports` iter order, `tlv_thunk_offsets[i]`, or
    /// `rebase_targets[i]`. The discriminant also picks the encoder
    /// — bind links carry the import ordinal; rebase links carry
    /// the 36-bit image-relative target.
    #[derive(Clone, Copy)]
    enum SlotKind {
        LaPtr(usize),
        TlvThunk(usize),
        Rebase(usize),
    }

    // Collect every chain participant as (offset_in_segment,
    // payload_data, kind). For bind slots payload_data is the
    // import ordinal (24-bit); for rebase slots payload_data is
    // the 36-bit target offset packed into u64. Ordinals follow
    // the imports[] layout: la-ptr ordinals 0..la_ptr_n, tlv
    // thunk ordinal = la_ptr_n. Rebase slots are encoded inline
    // (no import-table entry) so they share the slots[] vector
    // but bypass the bind-link path.
    let mut slots: Vec<(u64, u64, SlotKind)> =
        Vec::with_capacity(la_ptr_imports.len() + tlv_thunk_offsets.len() + rebase_targets.len());
    for i in 0..la_ptr_imports.len() {
        let off = la_ptr_offset_in_segment + (i as u64) * 8;
        slots.push((off, i as u64, SlotKind::LaPtr(i)));
    }
    for (i, &off) in tlv_thunk_offsets.iter().enumerate() {
        slots.push((off, u64::from(tlv_import_ordinal), SlotKind::TlvThunk(i)));
    }
    for (i, &(off, target)) in rebase_targets.iter().enumerate() {
        slots.push((off, target, SlotKind::Rebase(i)));
    }
    slots.sort_by_key(|&(off, _, _)| off);

    // Bucket sorted slots by __DATA page. page_count covers the
    // FILE-BACKED pages only — chunk 633: the kernel page-in
    // linker treats every page up to page_count as fixup-bearing
    // and faults it in from the file, so declaring the trailing
    // zerofill pages here replaced their kernel zero-fill with
    // whatever file bytes sit past filesize (the next segment's
    // LINKEDIT content — probe-proven: __bss statics read symtab
    // nlist bytes and a Mutex lock word came up nonzero → startup
    // hang). ld64 ground truth agrees: bun's __DATA is 410 vm
    // pages / 12 file pages and its page_count is 12. Chain slots
    // all live in the file region (asserted per-slot below), so
    // no coverage is lost.
    let page_count_u64 = segment_file_size.div_ceil(PAGE_SIZE);
    debug_assert!(
        page_count_u64 <= u64::from(u16::MAX),
        "page_count {page_count_u64} overflows u16 — __DATA filesize \
         {segment_file_size} too large for the chained-fixups encoding",
    );
    let page_count: u16 = page_count_u64 as u16;
    let mut page_starts: Vec<u16> = vec![DYLD_CHAINED_PTR_START_NONE; page_count as usize];

    // Per-slot encoded chain-link value, indexed by sorted-slot
    // order. After the chain walk we'll dispatch each back into
    // the la_ptr / tlv_thunk output vecs in input order.
    let mut sorted_link_values: Vec<u64> = vec![0; slots.len()];

    let mut i = 0usize;
    while i < slots.len() {
        let page_idx = (slots[i].0 / PAGE_SIZE) as usize;
        debug_assert!(
            page_idx < page_count as usize,
            "slot offset {} lands on page {page_idx} but page_count is {page_count}",
            slots[i].0,
        );
        // Span [i..j) covers every slot landing on this page —
        // already contiguous because slots is sorted by offset.
        let mut j = i + 1;
        while j < slots.len() && (slots[j].0 / PAGE_SIZE) as usize == page_idx {
            j += 1;
        }
        let page_base = (page_idx as u64) * PAGE_SIZE;
        page_starts[page_idx] = (slots[i].0 - page_base) as u16;
        // Link slots[i..j) via next_stride. The last slot in
        // the bucket terminates the chain (next_stride = 0).
        for k in i..j {
            let next_stride = if k + 1 < j {
                let delta = slots[k + 1].0 - slots[k].0;
                debug_assert!(
                    delta % 4 == 0,
                    "intra-page chain slots must be 4-byte aligned (delta {delta})",
                );
                let stride = delta / 4;
                debug_assert!(
                    stride < (1 << 12),
                    "intra-page next_stride {stride} overflows 12-bit field — \
                     bug in slot bucketing",
                );
                stride as u32
            } else {
                0
            };
            sorted_link_values[k] = match slots[k].2 {
                SlotKind::LaPtr(_) | SlotKind::TlvThunk(_) => {
                    // bind link: payload_data is the 24-bit import
                    // ordinal (cast back to u32 — collection above
                    // promised it fits).
                    encode_bind_link(slots[k].1 as u32, next_stride, 0)
                }
                SlotKind::Rebase(_) => {
                    // rebase link: payload_data is the 36-bit
                    // image-relative target offset.
                    encode_rebase_link(slots[k].1, next_stride, 0)
                }
            };
        }
        i = j;
    }

    // Dispatch encoded values back into per-kind output vecs in
    // the input order each caller iter expects.
    let mut la_ptr_slot_values: Vec<u64> = vec![0; la_ptr_imports.len()];
    let mut tlv_thunk_link_values: Vec<u64> = vec![0; tlv_thunk_offsets.len()];
    let mut rebase_link_values: Vec<u64> = vec![0; rebase_targets.len()];
    for (k, &(_, _, kind)) in slots.iter().enumerate() {
        match kind {
            SlotKind::LaPtr(idx) => la_ptr_slot_values[idx] = sorted_link_values[k],
            SlotKind::TlvThunk(idx) => tlv_thunk_link_values[idx] = sorted_link_values[k],
            SlotKind::Rebase(idx) => rebase_link_values[idx] = sorted_link_values[k],
        }
    }

    // Encode dyld_chained_starts_in_segment header + page_start[]
    // table.
    let starts_in_segment_size: u32 = 4   // size
                                    + 2   // page_size
                                    + 2   // pointer_format
                                    + 8   // segment_offset
                                    + 4   // max_valid_pointer
                                    + 2   // page_count
                                    + 2 * u32::from(page_count); // page_start[]
    let mut bytes: Vec<u8> = Vec::with_capacity(starts_in_segment_size as usize);
    bytes.extend_from_slice(&starts_in_segment_size.to_le_bytes());
    bytes.extend_from_slice(&(PAGE_SIZE as u16).to_le_bytes());
    bytes.extend_from_slice(&DYLD_CHAINED_PTR_64_OFFSET.to_le_bytes());
    bytes.extend_from_slice(&data_segment_vmaddr_offset.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes()); // max_valid_pointer
    bytes.extend_from_slice(&page_count.to_le_bytes());
    for ps in &page_starts {
        bytes.extend_from_slice(&ps.to_le_bytes());
    }

    StartsInSegment {
        bytes,
        la_ptr_slot_values,
        tlv_thunk_link_values,
        rebase_link_values,
    }
}

pub(crate) fn round_up_4(value: u32) -> u32 {
    (value + 3) & !3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imports_of(names: &[&str]) -> BTreeMap<String, u8> {
        names.iter().map(|s| ((*s).to_string(), 1u8)).collect()
    }

    /// Smoke test of the SD-4c-prereq+e7b-2 rebase path on its own —
    /// a single rebase slot at segment offset 0, no bind participants,
    /// chain terminates immediately.
    #[test]
    fn rebase_only_single_slot_chain_terminates() {
        let rebases = vec![(0u64, 0x4000u64)];
        let result = build_starts_in_segment(&BTreeMap::new(), 0, 0, PAGE_SIZE, &[], 0, &rebases);
        assert_eq!(result.rebase_link_values.len(), 1);
        // target = 0x4000, next = 0, high8 = 0, bind bit = 0.
        assert_eq!(result.rebase_link_values[0], 0x4000);
        // bind bit (63) cleared — this is what dyld dispatches on.
        assert_eq!(result.rebase_link_values[0] & (1u64 << 63), 0);
        assert!(result.la_ptr_slot_values.is_empty());
        assert!(result.tlv_thunk_link_values.is_empty());
    }

    /// Two rebase slots 8 bytes apart in the same page link into a
    /// single chain — first slot's next_stride = 2 (8 bytes / 4),
    /// last slot's next_stride = 0.
    #[test]
    fn rebase_two_slots_same_page_form_one_chain() {
        let rebases = vec![(0u64, 0x4000u64), (8u64, 0x5000u64)];
        let result = build_starts_in_segment(&BTreeMap::new(), 0, 0, PAGE_SIZE, &[], 0, &rebases);
        assert_eq!(result.rebase_link_values.len(), 2);
        // First slot: target 0x4000, next stride 2.
        let expected_first = 0x4000u64 | (2u64 << 51);
        assert_eq!(result.rebase_link_values[0], expected_first);
        // Last slot: target 0x5000, chain end.
        assert_eq!(result.rebase_link_values[1], 0x5000);
        // Both bind bits cleared.
        assert_eq!(result.rebase_link_values[0] & (1u64 << 63), 0);
        assert_eq!(result.rebase_link_values[1] & (1u64 << 63), 0);
    }

    /// Mixed bind + rebase slots in the same page bucket — bucketing
    /// orders by offset, encoder dispatches per SlotKind, and dyld
    /// gets a single chain that mixes bind (bit 63 = 1) and rebase
    /// (bit 63 = 0) entries linked by next_stride.
    #[test]
    fn mixed_bind_and_rebase_same_page_share_one_chain() {
        // __la_symbol_ptr placed first (la-ptr-offset 0, one slot),
        // a rebase target 8 bytes after it.
        let imports = imports_of(&["_malloc"]);
        let rebases = vec![(8u64, 0x4000u64)];
        let result = build_starts_in_segment(&imports, 0, 0, PAGE_SIZE, &[], 0, &rebases);

        // Bind slot first: next_stride 2 → links to rebase.
        assert_eq!(result.la_ptr_slot_values.len(), 1);
        let bind = result.la_ptr_slot_values[0];
        assert_eq!(bind & (1u64 << 63), 1u64 << 63, "bind bit must be set");
        assert_eq!((bind >> 51) & 0xFFF, 2, "next stride to rebase = 2");
        assert_eq!(bind & 0xFF_FFFF, 0, "import ordinal _malloc = 0");

        // Rebase slot: chain end, bind bit clear.
        assert_eq!(result.rebase_link_values.len(), 1);
        let rebase = result.rebase_link_values[0];
        assert_eq!(rebase & (1u64 << 63), 0, "rebase bit-63 must be clear");
        assert_eq!(rebase & 0xF_FFFF_FFFF, 0x4000, "target offset preserved");
        assert_eq!((rebase >> 51) & 0xFFF, 0, "rebase is chain end");
    }

    /// Rebase slots on two different pages each start their own chain
    /// — `page_start[]` records both page-relative offsets and neither
    /// chain crosses the page boundary.
    #[test]
    fn rebase_slots_in_two_pages_get_two_chain_heads() {
        let rebases = vec![(0u64, 0x4000u64), (PAGE_SIZE, 0x5000u64)];
        let result =
            build_starts_in_segment(&BTreeMap::new(), 0, 0, 2 * PAGE_SIZE, &[], 0, &rebases);
        // Both slots are chain ends — each in its own page bucket.
        assert_eq!(result.rebase_link_values.len(), 2);
        assert_eq!((result.rebase_link_values[0] >> 51) & 0xFFF, 0);
        assert_eq!((result.rebase_link_values[1] >> 51) & 0xFFF, 0);

        // Inspect page_start[] inside the encoded blob — bytes after
        // the 24-byte header.
        let header_size = 4 + 2 + 2 + 8 + 4 + 2;
        let page_starts = &result.bytes[header_size..];
        assert_eq!(
            u16::from_le_bytes(page_starts[0..2].try_into().unwrap()),
            0,
            "page 0 chain starts at offset 0",
        );
        assert_eq!(
            u16::from_le_bytes(page_starts[2..4].try_into().unwrap()),
            0,
            "page 1 chain starts at offset 0",
        );
    }
}
