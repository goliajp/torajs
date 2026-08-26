//! Byte payload emit — inner region (child_offsets globals + per-field
//! name strings + per-class FieldMeta arrays) + outer table + count.
//!
//! `class_layouts_link_values` is the chained-fixup link value slice
//! the e7b pipeline produced for this region's rebase targets. The
//! consumption order MUST match `compute_class_layouts_rebase_targets`:
//!   1. Per-class FieldMeta `name_ptr` slots (one LV per field, across
//!      all classes in entry-major order).
//!   2. Per-class outer-entry slots, in entry-major order: at most one
//!      `child_offsets_ptr` LV and one `field_metadata_ptr` LV per entry,
//!      skipped when the corresponding inner global is absent.

use super::types::{
    FIELD_META_FIELD_OFFSET_OFFSET_IN_ELEM, FIELD_META_NAME_LEN_OFFSET_IN_ELEM,
    FIELD_META_NAME_PTR_OFFSET_IN_ELEM, FIELD_META_TYPE_TAG_OFFSET_IN_ELEM,
    INNER_FIELD_META_ELEM_SIZE, INNER_METHOD_META_ELEM_SIZE,
    METHOD_META_ADAPTER_PTR_OFFSET_IN_ELEM, METHOD_META_NAME_LEN_OFFSET_IN_ELEM,
    METHOD_META_TWIN_PTR_OFFSET_IN_ELEM, UserClassLayoutsLayout,
};

pub fn build_user_class_layouts_payload(
    layout: &UserClassLayoutsLayout,
    class_layouts_link_values: &[u64],
) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::with_capacity(layout.total_size as usize);
    if layout.total_size == 0 {
        debug_assert!(class_layouts_link_values.is_empty());
        return buf;
    }
    let region_start = layout.file_offset;
    let mut link_idx = 0usize;

    // Phase 1: inner region per class.
    for entry in &layout.entries {
        // .__class_offsets_<i>
        if entry.inner_vaddr.is_some() {
            pad_buf_to(&mut buf, region_start, entry.inner_file_offset);
            for &off in &entry.child_offsets {
                buf.extend_from_slice(&off.to_le_bytes());
            }
        }
        // .__class_field_name_<i>_<j>
        for fm in &entry.field_metadata {
            pad_buf_to(&mut buf, region_start, fm.name_file_offset);
            buf.extend_from_slice(&fm.name_bytes);
        }
        // .__class_fields_<i> (header + body)
        if entry.field_meta_array_vaddr.is_some() {
            pad_buf_to(&mut buf, region_start, entry.field_meta_array_file_offset);
            // Header: u32 n_fields + u32 _pad.
            buf.extend_from_slice(&(entry.field_metadata.len() as u32).to_le_bytes());
            buf.extend_from_slice(&[0u8; 4]);
            // Body: N x FieldMeta (24B each).
            for fm in &entry.field_metadata {
                let elem_start = buf.len();
                // name_ptr u64 (chain-fixup link value).
                let lv = take_link_value(class_layouts_link_values, &mut link_idx);
                buf.extend_from_slice(&lv.to_le_bytes());
                debug_assert_eq!(
                    buf.len() - elem_start,
                    FIELD_META_NAME_LEN_OFFSET_IN_ELEM as usize
                );
                // name_len u32.
                buf.extend_from_slice(&(fm.name_bytes.len() as u32).to_le_bytes());
                debug_assert_eq!(
                    buf.len() - elem_start,
                    FIELD_META_FIELD_OFFSET_OFFSET_IN_ELEM as usize
                );
                // field_byte_offset u32.
                buf.extend_from_slice(&fm.field_byte_offset.to_le_bytes());
                debug_assert_eq!(
                    buf.len() - elem_start,
                    FIELD_META_TYPE_TAG_OFFSET_IN_ELEM as usize
                );
                // type_tag u8 + u8[7] pad.
                buf.push(fm.type_tag);
                buf.extend_from_slice(&[0u8; 7]);
                debug_assert_eq!(buf.len() - elem_start, INNER_FIELD_META_ELEM_SIZE as usize);
                let _ = FIELD_META_NAME_PTR_OFFSET_IN_ELEM; // = 0, asserted by construction
            }
        }
        // 刀 4 — .__class_method_name_<i>_<j> strings +
        // .__class_methods_<i> array, inside the SAME per-entry walk
        // (compute's inner cursor interleaves per class; buf writes
        // must stay strictly offset-ascending).
        for mm in &entry.methods {
            pad_buf_to(&mut buf, region_start, mm.name_file_offset);
            buf.extend_from_slice(&mm.name_bytes);
        }
        if entry.method_meta_array_vaddr.is_some() {
            pad_buf_to(&mut buf, region_start, entry.method_meta_array_file_offset);
            // Header: u32 n_methods + u32 _pad.
            buf.extend_from_slice(&(entry.methods.len() as u32).to_le_bytes());
            buf.extend_from_slice(&[0u8; 4]);
            // Body: N x MethodMeta (32B each).
            for mm in &entry.methods {
                let elem_start = buf.len();
                // name_ptr u64 (chain-fixup link value).
                let lv = take_link_value(class_layouts_link_values, &mut link_idx);
                buf.extend_from_slice(&lv.to_le_bytes());
                debug_assert_eq!(
                    buf.len() - elem_start,
                    METHOD_META_NAME_LEN_OFFSET_IN_ELEM as usize
                );
                // name_len u32 + flags u32 (S2.38 — bit 0 this-free).
                buf.extend_from_slice(&(mm.name_bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(&mm.flags.to_le_bytes());
                debug_assert_eq!(
                    buf.len() - elem_start,
                    METHOD_META_ADAPTER_PTR_OFFSET_IN_ELEM as usize
                );
                // adapter_ptr u64 (chain-fixup link value → fn vaddr);
                // r502: a blanked column bakes a raw 0 (lockstep with
                // the sym.rs enumeration).
                let lv = if mm.adapter_fn_id.is_some() {
                    take_link_value(class_layouts_link_values, &mut link_idx)
                } else {
                    0
                };
                buf.extend_from_slice(&lv.to_le_bytes());
                debug_assert_eq!(
                    buf.len() - elem_start,
                    METHOD_META_TWIN_PTR_OFFSET_IN_ELEM as usize
                );
                // twin_ptr u64 — blade 3: a minted twin consumes a
                // link value (lockstep with the sym.rs enumeration);
                // no twin bakes a raw 0.
                let twin_lv = if mm.twin_fn_id.is_some() {
                    take_link_value(class_layouts_link_values, &mut link_idx)
                } else {
                    0
                };
                buf.extend_from_slice(&twin_lv.to_le_bytes());
                debug_assert_eq!(buf.len() - elem_start, INNER_METHOD_META_ELEM_SIZE as usize);
            }
        }
    }

    // Phase 2: outer table.
    pad_buf_to(&mut buf, region_start, layout.outer_file_offset);
    for entry in &layout.entries {
        buf.extend_from_slice(&entry.n_children.to_le_bytes());
        // L3b #4 — flags word (bit 0 = named class) in the former pad.
        buf.extend_from_slice(&entry.flags.to_le_bytes());
        let child_offsets_lv: u64 = if entry.inner_vaddr.is_some() {
            take_link_value(class_layouts_link_values, &mut link_idx)
        } else {
            0
        };
        buf.extend_from_slice(&child_offsets_lv.to_le_bytes());
        let field_meta_lv: u64 = if entry.field_meta_array_vaddr.is_some() {
            take_link_value(class_layouts_link_values, &mut link_idx)
        } else {
            0
        };
        buf.extend_from_slice(&field_meta_lv.to_le_bytes());
        // 刀 4 — method_table_ptr slot (+24).
        let method_table_lv: u64 = if entry.method_meta_array_vaddr.is_some() {
            take_link_value(class_layouts_link_values, &mut link_idx)
        } else {
            0
        };
        buf.extend_from_slice(&method_table_lv.to_le_bytes());
    }
    debug_assert_eq!(
        link_idx,
        class_layouts_link_values.len(),
        "class_layouts_link_values carries extras the layout did not consume — caller misalignment",
    );

    // Phase 3: count.
    pad_buf_to(&mut buf, region_start, layout.count_file_offset);
    buf.extend_from_slice(&(layout.entries.len() as u32).to_le_bytes());

    debug_assert_eq!(buf.len() as u32, layout.total_size);
    buf
}

fn pad_buf_to(buf: &mut Vec<u8>, region_start: u32, target_file_offset: u32) {
    debug_assert!(
        target_file_offset >= region_start,
        "target offset {target_file_offset:#x} precedes region start {region_start:#x}",
    );
    let target = (target_file_offset - region_start) as usize;
    while buf.len() < target {
        buf.push(0);
    }
}

fn take_link_value(values: &[u64], idx: &mut usize) -> u64 {
    let v = *values.get(*idx).unwrap_or_else(|| {
        panic!(
            "class_layouts_link_values too short — expected at least {} entries, got {}",
            *idx + 1,
            values.len(),
        )
    });
    *idx += 1;
    v
}
