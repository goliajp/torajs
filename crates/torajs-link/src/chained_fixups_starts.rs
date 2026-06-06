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
};

/// Pre-encoded `dyld_chained_starts_in_segment` blob together
/// with the per-slot 8-byte chain link values the emit pass
/// writes into `__DATA,__la_symbol_ptr` and each TLV descriptor's
/// `thunk` slot. Split out so [`crate::chained_fixups::build_chained_fixups`]
/// stays under the function-size limit while the per-page chain math
/// gets dedicated coverage.
pub(crate) struct StartsInSegment {
    pub(crate) bytes: Vec<u8>,
    pub(crate) la_ptr_slot_values: Vec<u64>,
    pub(crate) tlv_thunk_link_values: Vec<u64>,
}

/// Encode the `dyld_chained_starts_in_segment` blob for `__DATA`,
/// bucketing every chain participant into its `__DATA` page and
/// linking each bucket's slots via `next_stride`. The per-slot
/// chain link values are returned alongside so the emit pass can
/// write them into the binary at the matching offsets.
pub(crate) fn build_starts_in_segment(
    la_ptr_imports: &BTreeMap<String, u8>,
    la_ptr_offset_in_segment: u64,
    data_segment_vmaddr_offset: u64,
    data_segment_vmsize: u64,
    tlv_thunk_offsets: &[u64],
    tlv_import_ordinal: u32,
) -> StartsInSegment {
    /// Identifies which output vec a slot's encoded chain link
    /// belongs to so callers can match it back to the input
    /// `la_ptr_imports` iter order or `tlv_thunk_offsets[i]`.
    #[derive(Clone, Copy)]
    enum SlotKind {
        LaPtr(usize),
        TlvThunk(usize),
    }

    // Collect every chain participant as (offset_in_segment,
    // import_ordinal, kind). Ordinals follow the imports[]
    // layout choice above: la-ptr ordinals 0..la_ptr_n, tlv
    // thunk ordinal = la_ptr_n.
    let mut slots: Vec<(u64, u32, SlotKind)> =
        Vec::with_capacity(la_ptr_imports.len() + tlv_thunk_offsets.len());
    for i in 0..la_ptr_imports.len() {
        let off = la_ptr_offset_in_segment + (i as u64) * 8;
        slots.push((off, i as u32, SlotKind::LaPtr(i)));
    }
    for (i, &off) in tlv_thunk_offsets.iter().enumerate() {
        slots.push((off, tlv_import_ordinal, SlotKind::TlvThunk(i)));
    }
    slots.sort_by_key(|&(off, _, _)| off);

    // Bucket sorted slots by __DATA page. page_count covers the
    // full vmsize so trailing zerofill pages still appear (with
    // the empty-page sentinel) — dyld walks all page_start[]
    // entries up to page_count regardless of where slots live.
    let page_count_u64 = data_segment_vmsize.div_ceil(PAGE_SIZE);
    debug_assert!(
        page_count_u64 <= u64::from(u16::MAX),
        "page_count {page_count_u64} overflows u16 — __DATA vmsize \
         {data_segment_vmsize} too large for the chained-fixups encoding",
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
            sorted_link_values[k] = encode_bind_link(slots[k].1, next_stride, 0);
        }
        i = j;
    }

    // Dispatch encoded values back into per-kind output vecs in
    // the input order each caller iter expects.
    let mut la_ptr_slot_values: Vec<u64> = vec![0; la_ptr_imports.len()];
    let mut tlv_thunk_link_values: Vec<u64> = vec![0; tlv_thunk_offsets.len()];
    for (k, &(_, _, kind)) in slots.iter().enumerate() {
        match kind {
            SlotKind::LaPtr(idx) => la_ptr_slot_values[idx] = sorted_link_values[k],
            SlotKind::TlvThunk(idx) => tlv_thunk_link_values[idx] = sorted_link_values[k],
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
    }
}

pub(crate) fn round_up_4(value: u32) -> u32 {
    (value + 3) & !3
}
