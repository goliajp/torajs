//! Per-class placement math — inner child_offsets globals + W-J A3b
//! per-field name strings + per-class FieldMeta array, then outer
//! table, then count.

use super::types::{
    COUNT_ALIGN, COUNT_SIZE, ENTRY_FLAG_GENERIC_ROW, ENTRY_FLAG_NAMED_CLASS, INNER_ELEM_ALIGN,
    INNER_ELEM_SIZE, INNER_FIELD_META_ALIGN, INNER_FIELD_META_ELEM_SIZE,
    INNER_FIELD_META_HEADER_SIZE, INNER_METHOD_META_ALIGN, INNER_METHOD_META_ELEM_SIZE,
    INNER_METHOD_META_HEADER_SIZE, OUTER_ENTRY_ALIGN, OUTER_ENTRY_SIZE, UserClassLayoutEntryLayout,
    UserClassLayoutsLayout, UserFieldMetaPlacement, UserMethodMetaPlacement,
};
use crate::exec::UserClassLayoutEntry;

/// Compute placements for the inner globals, outer table, and count
/// global. Returns an empty layout (no entries, `total_size = 0`) when
/// `class_layouts` is empty so callers can short-circuit before
/// touching any segment bytes.
pub fn compute_user_class_layouts_layout(
    class_layouts: &[UserClassLayoutEntry],
    cursor: u32,
    vaddr_base: u64,
    force_emit_globals: bool,
) -> UserClassLayoutsLayout {
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

    // Inner region starts at 4-align (child_offsets's tighter requirement
    // is 4; per-field FieldMeta array bumps to 8 within the region).
    let align_pad_to_inner = pad_to(cursor, INNER_ELEM_ALIGN);
    let region_file_offset = cursor + align_pad_to_inner;
    let region_vaddr = vaddr_base + u64::from(align_pad_to_inner);

    let mut entries: Vec<UserClassLayoutEntryLayout> = Vec::with_capacity(class_layouts.len());
    // Inner region cursor relative to `region_file_offset` / `region_vaddr`.
    let mut inner_cursor: u32 = 0;
    for entry in class_layouts {
        let n_children = entry.child_offsets.len() as u32;
        // .__class_offsets_<i>  ([K x i32], 4-align)
        let (inner_vaddr, inner_file_offset) = if entry.child_offsets.is_empty() {
            (None, 0u32)
        } else {
            let pad = pad_to(inner_cursor, INNER_ELEM_ALIGN);
            inner_cursor += pad;
            let inner_file_offset = region_file_offset + inner_cursor;
            let inner_vaddr = region_vaddr + u64::from(inner_cursor);
            inner_cursor += n_children * INNER_ELEM_SIZE;
            (Some(inner_vaddr), inner_file_offset)
        };

        // Per-field name byte strings (.__class_field_name_<i>_<j>,
        // 1-align). Place them before the FieldMeta array so the
        // array's name_ptr slots have higher file offsets than their
        // targets (file-byte ascending rebase layout).
        let mut field_metadata: Vec<UserFieldMetaPlacement> =
            Vec::with_capacity(entry.fields.len());
        for fm in &entry.fields {
            let name_bytes = fm.name.clone();
            let name_byte_len = name_bytes.len() as u32;
            let name_file_offset = region_file_offset + inner_cursor;
            let name_vaddr = region_vaddr + u64::from(inner_cursor);
            inner_cursor += name_byte_len;
            field_metadata.push(UserFieldMetaPlacement {
                name_bytes,
                name_vaddr,
                name_file_offset,
                field_byte_offset: fm.offset,
                type_tag: fm.type_tag,
            });
        }

        // .__class_fields_<i>  (header 8B + N x 24B, 8-align).
        let (field_meta_array_vaddr, field_meta_array_file_offset) = if entry.fields.is_empty() {
            (None, 0u32)
        } else {
            let pad = pad_to(inner_cursor, INNER_FIELD_META_ALIGN);
            inner_cursor += pad;
            let array_file_offset = region_file_offset + inner_cursor;
            let array_vaddr = region_vaddr + u64::from(inner_cursor);
            inner_cursor += INNER_FIELD_META_HEADER_SIZE
                + (entry.fields.len() as u32) * INNER_FIELD_META_ELEM_SIZE;
            (Some(array_vaddr), array_file_offset)
        };

        // 刀 4 — per-method name byte strings (mirrors the field-name
        // placement above: names precede the MethodMeta array).
        let mut methods: Vec<UserMethodMetaPlacement> = Vec::with_capacity(entry.methods.len());
        for mm in &entry.methods {
            let name_bytes = mm.name.clone();
            let name_byte_len = name_bytes.len() as u32;
            let name_file_offset = region_file_offset + inner_cursor;
            let name_vaddr = region_vaddr + u64::from(inner_cursor);
            inner_cursor += name_byte_len;
            methods.push(UserMethodMetaPlacement {
                name_bytes,
                name_vaddr,
                name_file_offset,
                adapter_fn_id: mm.adapter_fn_id,
                twin_fn_id: mm.twin_fn_id,
                flags: mm.flags,
            });
        }

        // .__class_methods_<i>  (header 8B + N x 24B, 8-align).
        let (method_meta_array_vaddr, method_meta_array_file_offset) = if entry.methods.is_empty() {
            (None, 0u32)
        } else {
            let pad = pad_to(inner_cursor, INNER_METHOD_META_ALIGN);
            inner_cursor += pad;
            let array_file_offset = region_file_offset + inner_cursor;
            let array_vaddr = region_vaddr + u64::from(inner_cursor);
            inner_cursor += INNER_METHOD_META_HEADER_SIZE
                + (entry.methods.len() as u32) * INNER_METHOD_META_ELEM_SIZE;
            (Some(array_vaddr), array_file_offset)
        };

        entries.push(UserClassLayoutEntryLayout {
            n_children,
            child_offsets: entry.child_offsets.clone(),
            flags: if entry.is_named {
                ENTRY_FLAG_NAMED_CLASS
            } else {
                0
            } | if entry.is_generic {
                ENTRY_FLAG_GENERIC_ROW
            } else {
                0
            },
            inner_vaddr,
            inner_file_offset,
            field_metadata,
            field_meta_array_vaddr,
            field_meta_array_file_offset,
            methods,
            method_meta_array_vaddr,
            method_meta_array_file_offset,
            entry_vaddr: 0, // filled in below once outer placement is known
            entry_file_offset: 0,
        });
    }

    // Outer table — 8-aligned past the inner region.
    let inner_end_file_offset = region_file_offset + inner_cursor;
    let inner_end_vaddr = region_vaddr + u64::from(inner_cursor);
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
/// compute the layout in one call.
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
