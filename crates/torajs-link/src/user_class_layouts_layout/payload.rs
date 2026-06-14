//! Byte payload emit — inner globals, outer-table entries, count.

use super::types::UserClassLayoutsLayout;

/// Build the contiguous byte payload: inner globals, leading pad,
/// outer-table entries (each filled from `outer_inner_link_values`
/// for entries with a `Some(inner_vaddr)`; `None` entries pack
/// `(0, NULL)`), trailing pad, then the count u32. `outer_inner_link_values`
/// carries one entry per non-empty `child_offsets` — the chained-fixup
/// link value the e7b path produces for each inner-ptr rebase target.
///
/// Returns a buffer of exactly `layout.total_size` bytes. The caller
/// splices it into `__DATA_CONST` at `layout.file_offset`.
pub fn build_user_class_layouts_payload(
    layout: &UserClassLayoutsLayout,
    outer_inner_link_values: &[u64],
) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::with_capacity(layout.total_size as usize);
    if layout.total_size == 0 {
        debug_assert!(outer_inner_link_values.is_empty());
        return buf;
    }
    // Inner globals — concatenated `[K x i32]` little-endian.
    for entry in &layout.entries {
        if entry.inner_vaddr.is_none() {
            continue;
        }
        for &off in &entry.child_offsets {
            buf.extend_from_slice(&off.to_le_bytes());
        }
    }
    // Pad to outer-table alignment.
    while buf.len() < (layout.outer_file_offset - layout.file_offset) as usize {
        buf.push(0);
    }
    // Outer table — 24 bytes per entry: u32 n + 4 pad + u64
    // inner_link_value + u64 field_metadata_link_value. W-J Phase A2:
    // field_metadata is a NULL stub (Phase A3 will populate it);
    // since it's NULL there's no chained-fixup rebase target for it
    // (the rebase target list still has one entry per non-empty
    // child_offsets — compute_class_layouts_rebase_targets unchanged).
    let mut link_idx = 0usize;
    for entry in &layout.entries {
        buf.extend_from_slice(&entry.n_children.to_le_bytes());
        buf.extend_from_slice(&[0u8; 4]);
        let link_value: u64 = if entry.inner_vaddr.is_some() {
            let v = *outer_inner_link_values.get(link_idx).unwrap_or_else(|| {
                panic!(
                    "outer_inner_link_values too short — expected at least {} entries, got {}",
                    link_idx + 1,
                    outer_inner_link_values.len(),
                )
            });
            link_idx += 1;
            v
        } else {
            0
        };
        buf.extend_from_slice(&link_value.to_le_bytes());
        // W-J Phase A2: field_metadata_ptr stub (NULL).
        buf.extend_from_slice(&0u64.to_le_bytes());
    }
    debug_assert_eq!(
        link_idx,
        outer_inner_link_values.len(),
        "outer_inner_link_values carries extras the layout did not consume — caller misalignment",
    );
    // Pad to count alignment.
    while buf.len() < (layout.count_file_offset - layout.file_offset) as usize {
        buf.push(0);
    }
    // Count u32.
    buf.extend_from_slice(&(layout.entries.len() as u32).to_le_bytes());
    debug_assert_eq!(buf.len() as u32, layout.total_size);
    buf
}
