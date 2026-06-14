//! Per-class placement math — inner globals, outer table, count.

use super::types::{
    COUNT_ALIGN, COUNT_SIZE, INNER_ELEM_ALIGN, INNER_ELEM_SIZE, OUTER_ENTRY_ALIGN,
    OUTER_ENTRY_SIZE, UserClassLayoutEntryLayout, UserClassLayoutsLayout,
};
use crate::exec::UserClassLayoutEntry;

/// Compute placements for the inner globals, outer table, and count
/// global. Returns an empty layout (no entries, `total_size = 0`) when
/// `class_layouts` is empty so callers can short-circuit before
/// touching any segment bytes (mirrors `compute_user_vtables_layout`).
pub fn compute_user_class_layouts_layout(
    class_layouts: &[UserClassLayoutEntry],
    cursor: u32,
    vaddr_base: u64,
    force_emit_globals: bool,
) -> UserClassLayoutsLayout {
    // class_layouts empty + force_emit_globals=false → no bytes, no
    // sym registration. Self-contained probes (no `libtorajs_cycle.a`
    // dep) take this path and stay byte-identical to the pre-e8
    // baseline.
    //
    // class_layouts empty + force_emit_globals=true → emit a 4-byte
    // count global (= 0) and register both syms aliased to it; the
    // empty outer table has zero bytes so `outer_vaddr` sharing the
    // count slot is safe (runtime reads count first, only walks the
    // outer table when N > 0). `tr build` takes this path because
    // every staticlib bundle pulls `libtorajs_cycle.a`, which
    // unconditionally references these syms.
    if class_layouts.is_empty() && !force_emit_globals {
        return UserClassLayoutsLayout {
            entries: Vec::new(),
            file_offset: cursor,
            vaddr: vaddr_base,
            outer_vaddr: vaddr_base,
            outer_file_offset: cursor,
            count_vaddr: vaddr_base,
            count_file_offset: cursor,
            total_size: 0,
        };
    }
    // Inner globals come first — they need 4-byte alignment, which
    // matches the caller's cursor when the prior region ended on a
    // 4-aligned boundary (vtables end on an 8-aligned boundary, which
    // is also 4-aligned).
    let align_pad_to_inner = pad_to(cursor, INNER_ELEM_ALIGN);
    let region_file_offset = cursor + align_pad_to_inner;
    let region_vaddr = vaddr_base + u64::from(align_pad_to_inner);

    let mut entries: Vec<UserClassLayoutEntryLayout> = Vec::with_capacity(class_layouts.len());
    let mut cumulative: u32 = 0;
    for entry in class_layouts {
        let n_children = entry.child_offsets.len() as u32;
        let (inner_vaddr, inner_file_offset, inner_size) = if entry.child_offsets.is_empty() {
            (None, 0u32, 0u32)
        } else {
            let inner_file_offset = region_file_offset + cumulative;
            let inner_vaddr = region_vaddr + u64::from(cumulative);
            (
                Some(inner_vaddr),
                inner_file_offset,
                n_children * INNER_ELEM_SIZE,
            )
        };
        cumulative += inner_size;
        entries.push(UserClassLayoutEntryLayout {
            n_children,
            child_offsets: entry.child_offsets.clone(),
            inner_vaddr,
            inner_file_offset,
            entry_vaddr: 0, // filled in below once outer placement is known
            entry_file_offset: 0,
        });
    }

    // Outer table — 8-aligned past the inner globals.
    let inner_end_file_offset = region_file_offset + cumulative;
    let inner_end_vaddr = region_vaddr + u64::from(cumulative);
    let outer_align_pad = pad_to(inner_end_file_offset, OUTER_ENTRY_ALIGN);
    let outer_file_offset = inner_end_file_offset + outer_align_pad;
    let outer_vaddr = inner_end_vaddr + u64::from(outer_align_pad);
    for (i, slot) in entries.iter_mut().enumerate() {
        slot.entry_file_offset = outer_file_offset + (i as u32) * OUTER_ENTRY_SIZE;
        slot.entry_vaddr = outer_vaddr + (i as u64) * u64::from(OUTER_ENTRY_SIZE);
    }
    let outer_size = entries.len() as u32 * OUTER_ENTRY_SIZE;

    // Count global — 4-aligned past the outer table.
    let count_align_pad = pad_to(outer_file_offset + outer_size, COUNT_ALIGN);
    let count_file_offset = outer_file_offset + outer_size + count_align_pad;
    let count_vaddr = outer_vaddr + u64::from(outer_size + count_align_pad);
    let total_size = (count_file_offset + COUNT_SIZE) - cursor;

    UserClassLayoutsLayout {
        entries,
        file_offset: region_file_offset,
        vaddr: region_vaddr,
        outer_vaddr,
        outer_file_offset,
        count_vaddr,
        count_file_offset,
        total_size,
    }
}

pub(super) fn pad_to(cursor: u32, align: u32) -> u32 {
    debug_assert!(
        align.is_power_of_two(),
        "align {align} must be a power of two"
    );
    (align - (cursor % align)) % align
}

/// Pipeline helper — derive vaddr_base from segment base + cursor and
/// compute the layout in one call. Mirrors `build_user_vtables_region`.
pub fn build_user_class_layouts_region(
    class_layouts: &[UserClassLayoutEntry],
    seg_vmaddr_base: u64,
    file_offset: u32,
    seg_file_offset_base: u32,
    force_emit_globals: bool,
) -> UserClassLayoutsLayout {
    let vaddr_base = seg_vmaddr_base + u64::from(file_offset - seg_file_offset_base);
    compute_user_class_layouts_layout(class_layouts, file_offset, vaddr_base, force_emit_globals)
}
