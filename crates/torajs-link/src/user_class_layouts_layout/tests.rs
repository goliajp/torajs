//! Tests for the class-layouts region — exercise via the public API
//! re-exported from the parent module.

use super::*;
use crate::exec::{UserClassLayoutEntry, UserFieldMetaEntry};
use crate::resolve::SymTable;

fn entry(offsets: &[u32]) -> UserClassLayoutEntry {
    UserClassLayoutEntry {
        child_offsets: offsets.to_vec(),
        fields: Vec::new(),
        is_named: true,
        methods: Vec::new(),
    }
}

fn fmeta(name: &str, offset: u32, type_tag: u8) -> UserFieldMetaEntry {
    UserFieldMetaEntry {
        name: name.into(),
        offset,
        type_tag,
    }
}

fn entry_with_fields(offsets: &[u32], fields: Vec<UserFieldMetaEntry>) -> UserClassLayoutEntry {
    UserClassLayoutEntry {
        child_offsets: offsets.to_vec(),
        fields,
        is_named: true,
        methods: Vec::new(),
    }
}

#[test]
fn empty_input_zero_total() {
    let layout = compute_user_class_layouts_layout(&[], 0x4000, 0x1_0000_4000, false);
    assert!(layout.entries.is_empty());
    assert_eq!(layout.total_size, 0);
}

#[test]
fn empty_input_force_emit_yields_4byte_count_global() {
    let layout = compute_user_class_layouts_layout(&[], 0x4000, 0x1_0000_4000, true);
    assert!(layout.entries.is_empty());
    assert_eq!(layout.total_size, 4);
    assert_eq!(layout.outer_vaddr, 0x1_0000_4000);
    assert_eq!(layout.count_vaddr, 0x1_0000_4000);

    let payload = build_user_class_layouts_payload(&layout, &[]);
    assert_eq!(payload, vec![0u8, 0, 0, 0]);

    let mut sym_table = SymTable::new();
    apply_user_class_layouts_overrides(&layout, &mut sym_table);
    assert_eq!(sym_table.get(CLASS_LAYOUTS_SYM), Some(&0x1_0000_4000));
    assert_eq!(sym_table.get(N_CLASS_LAYOUTS_SYM), Some(&0x1_0000_4000));
}

#[test]
fn single_entry_two_offsets_no_fields() {
    let class_layouts = [entry(&[24, 32])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    assert_eq!(layout.entries.len(), 1);
    assert_eq!(layout.entries[0].inner_file_offset, 0x4000);
    assert_eq!(layout.entries[0].inner_vaddr, Some(0x1_0000_4000));
    // No fields → no .__class_fields_<i> inner.
    assert!(layout.entries[0].field_meta_array_vaddr.is_none());
    assert!(layout.entries[0].field_metadata.is_empty());
    // Outer past 8-byte inner.
    assert_eq!(layout.outer_file_offset, 0x4008);
    assert_eq!(layout.outer_vaddr, 0x1_0000_4008);
    assert_eq!(layout.count_file_offset, 0x4028);
    assert_eq!(layout.count_vaddr, 0x1_0000_4028);
    // Total = 8 (inner) + 32 (outer) + 4 (count) = 44.
    assert_eq!(layout.total_size, 44);
}

#[test]
fn empty_child_offsets_no_fields_skips_all_inner() {
    let class_layouts = [entry(&[])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    assert!(layout.entries[0].inner_vaddr.is_none());
    assert!(layout.entries[0].field_meta_array_vaddr.is_none());
    assert_eq!(layout.outer_file_offset, 0x4000);
    // 32 (outer) + 4 (count) = 36.
    assert_eq!(layout.total_size, 36);
}

#[test]
fn mixed_empty_and_nonempty_pack_back_to_back() {
    let class_layouts = [entry(&[16]), entry(&[]), entry(&[32, 40])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    assert_eq!(layout.entries[0].inner_file_offset, 0x4000);
    assert_eq!(layout.entries[0].inner_vaddr, Some(0x1_0000_4000));
    assert!(layout.entries[1].inner_vaddr.is_none());
    // Entry 2 inner sits at 0x4004 (right after entry 0's 4 bytes; entry
    // 0's child_offsets is 4-aligned already, no pad needed).
    assert_eq!(layout.entries[2].inner_file_offset, 0x4004);
    // Total inner = 4 + 8 = 12 bytes; outer 8-align pad = 4 bytes.
    assert_eq!(layout.outer_file_offset, 0x4010);
    // 3 outer entries × 32 = 96 bytes; count at 0x4070.
    assert_eq!(layout.count_file_offset, 0x4070);
    assert_eq!(layout.total_size, 4 + 8 + 4 + 96 + 4); // 116
}

#[test]
fn build_payload_emits_inner_outer_count_in_order_no_fields() {
    let class_layouts = [entry(&[16, 24])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    let link_values = [0xDEAD_BEEFu64];
    let payload = build_user_class_layouts_payload(&layout, &link_values);
    // 8 inner + 32 outer + 4 count = 44.
    assert_eq!(payload.len(), 44);
    // [0..8]   inner [16, 24] LE
    assert_eq!(u32::from_le_bytes(payload[0..4].try_into().unwrap()), 16);
    assert_eq!(u32::from_le_bytes(payload[4..8].try_into().unwrap()), 24);
    // [8..12]  outer entry n_children = 2
    assert_eq!(u32::from_le_bytes(payload[8..12].try_into().unwrap()), 2);
    // [12..16] outer entry flags = NAMED_CLASS (helper builds named)
    assert_eq!(
        u32::from_le_bytes(payload[12..16].try_into().unwrap()),
        ENTRY_FLAG_NAMED_CLASS
    );
    // [16..24] outer entry child_offsets_ptr = link value
    assert_eq!(
        u64::from_le_bytes(payload[16..24].try_into().unwrap()),
        0xDEAD_BEEF
    );
    // [24..32] outer entry field_metadata_ptr = NULL (no fields)
    assert_eq!(u64::from_le_bytes(payload[24..32].try_into().unwrap()), 0);
    // [32..40] outer entry method_table_ptr = NULL (no methods)
    assert_eq!(u64::from_le_bytes(payload[32..40].try_into().unwrap()), 0);
    // [40..44] count = 1
    assert_eq!(u32::from_le_bytes(payload[40..44].try_into().unwrap()), 1);
}

#[test]
fn build_payload_zeroes_empty_entry_outer_slot() {
    let class_layouts = [entry(&[])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    let payload = build_user_class_layouts_payload(&layout, &[]);
    // 32 outer + 4 count = 36.
    assert_eq!(payload.len(), 36);
    assert_eq!(u32::from_le_bytes(payload[0..4].try_into().unwrap()), 0);
    assert_eq!(u64::from_le_bytes(payload[8..16].try_into().unwrap()), 0);
    assert_eq!(u64::from_le_bytes(payload[16..24].try_into().unwrap()), 0);
    assert_eq!(u64::from_le_bytes(payload[24..32].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(payload[32..36].try_into().unwrap()), 1);
}

#[test]
fn apply_overrides_inserts_both_syms() {
    let class_layouts = [entry(&[16])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    let mut sym_table = SymTable::new();
    apply_user_class_layouts_overrides(&layout, &mut sym_table);
    assert_eq!(sym_table.get(CLASS_LAYOUTS_SYM), Some(&layout.outer_vaddr));
    assert_eq!(
        sym_table.get(N_CLASS_LAYOUTS_SYM),
        Some(&layout.count_vaddr)
    );
}

#[test]
fn apply_overrides_skips_empty_input() {
    let layout = compute_user_class_layouts_layout(&[], 0x4000, 0x1_0000_4000, false);
    let mut sym_table = SymTable::new();
    apply_user_class_layouts_overrides(&layout, &mut sym_table);
    assert!(sym_table.get(CLASS_LAYOUTS_SYM).is_none());
    assert!(sym_table.get(N_CLASS_LAYOUTS_SYM).is_none());
}

#[test]
fn extra_defined_syms_two_when_nonempty_or_force_emit() {
    let empty = user_class_layouts_extra_defined_syms(&[], false);
    assert!(empty.is_empty());
    let forced = user_class_layouts_extra_defined_syms(&[], true);
    assert_eq!(forced.len(), 2);
    assert!(forced.contains(CLASS_LAYOUTS_SYM));
    assert!(forced.contains(N_CLASS_LAYOUTS_SYM));
    let class_layouts = [entry(&[16])];
    let some = user_class_layouts_extra_defined_syms(&class_layouts, false);
    assert_eq!(some.len(), 2);
    assert!(some.contains(CLASS_LAYOUTS_SYM));
    assert!(some.contains(N_CLASS_LAYOUTS_SYM));
}

#[test]
fn rebase_targets_only_child_offsets_when_no_fields() {
    let class_layouts = [entry(&[16]), entry(&[]), entry(&[24, 32])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    let targets = compute_class_layouts_rebase_targets(&layout, &[], 0x1_0000_0000, 0x1_0000_0000);
    // 2 non-empty entries → 2 rebase targets (no fields, no name_ptr
    // rebases, no field_metadata_ptr rebases).
    assert_eq!(targets.len(), 2);
    let entry0_slot_off = (layout.entries[0].entry_vaddr - 0x1_0000_0000) + 8;
    let entry0_target_off = layout.entries[0].inner_vaddr.unwrap() - 0x1_0000_0000;
    assert_eq!(targets[0], (entry0_slot_off, entry0_target_off));
    let entry2_slot_off = (layout.entries[2].entry_vaddr - 0x1_0000_0000) + 8;
    let entry2_target_off = layout.entries[2].inner_vaddr.unwrap() - 0x1_0000_0000;
    assert_eq!(targets[1], (entry2_slot_off, entry2_target_off));
}

// ---------------- W-J A3b — field metadata tests ----------------

#[test]
fn single_entry_with_one_field_layout_shape() {
    // 1 class, 0 child_offsets, 1 field "x" at offset 32, type_tag 1.
    // Inner region: name string "x" (1B) + pad to 8 (7B) + field_meta
    //   header (8B) + 1 FieldMeta (24B) = 40B.
    // Outer: 32B. Count: 4B. Total = 40 + 32 + 4 = 76.
    let fields = vec![fmeta("x", 32, 1)];
    let class_layouts = [entry_with_fields(&[], fields)];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    assert_eq!(layout.entries.len(), 1);
    let e = &layout.entries[0];
    assert!(e.inner_vaddr.is_none()); // no child_offsets
    assert_eq!(e.field_metadata.len(), 1);
    assert_eq!(e.field_metadata[0].name_bytes, b"x");
    assert_eq!(e.field_metadata[0].name_byte_offset_check(), 0x4000);
    // Field_meta array sits at 0x4008 (after name+pad).
    assert_eq!(e.field_meta_array_file_offset, 0x4008);
    assert_eq!(e.field_meta_array_vaddr, Some(0x1_0000_4008));
    // Outer past inner = 0x4008 + 8 + 24 = 0x4028.
    assert_eq!(layout.outer_file_offset, 0x4028);
    // Count past outer = 0x4028 + 32 = 0x4048.
    assert_eq!(layout.count_file_offset, 0x4048);
    assert_eq!(layout.total_size, 40 + 32 + 4);
}

#[test]
fn single_entry_with_fields_payload_emits_in_order() {
    let fields = vec![fmeta("x", 32, 1)];
    let class_layouts = [entry_with_fields(&[], fields)];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    // Two link values consumed: 1 name_ptr (FieldMeta x.name) + 1
    // outer field_metadata_ptr (entry.field_meta_array_vaddr). No
    // child_offsets_ptr (entry has no child_offsets).
    let lvs = [0xAAAAu64, 0xBBBBu64];
    let payload = build_user_class_layouts_payload(&layout, &lvs);
    assert_eq!(payload.len() as u32, layout.total_size);
    // [0..1]   name "x"
    assert_eq!(&payload[0..1], b"x");
    // [1..8]   pad to 8-align before field_meta array
    assert_eq!(&payload[1..8], &[0u8; 7]);
    // [8..12]  FieldMeta array header: n_fields = 1
    assert_eq!(u32::from_le_bytes(payload[8..12].try_into().unwrap()), 1);
    // [12..16] header pad
    assert_eq!(u32::from_le_bytes(payload[12..16].try_into().unwrap()), 0);
    // [16..24] FieldMeta name_ptr = 0xAAAA (LV)
    assert_eq!(
        u64::from_le_bytes(payload[16..24].try_into().unwrap()),
        0xAAAA
    );
    // [24..28] FieldMeta name_len = 1
    assert_eq!(u32::from_le_bytes(payload[24..28].try_into().unwrap()), 1);
    // [28..32] FieldMeta field_byte_offset = 32
    assert_eq!(u32::from_le_bytes(payload[28..32].try_into().unwrap()), 32);
    // [32]     FieldMeta type_tag = 1
    assert_eq!(payload[32], 1);
    // [33..40] FieldMeta tail pad
    assert_eq!(&payload[33..40], &[0u8; 7]);
    // [40..44] outer entry n_children = 0
    assert_eq!(u32::from_le_bytes(payload[40..44].try_into().unwrap()), 0);
    // [44..48] outer entry flags = NAMED_CLASS (helper builds named)
    assert_eq!(
        u32::from_le_bytes(payload[44..48].try_into().unwrap()),
        ENTRY_FLAG_NAMED_CLASS
    );
    // [48..56] outer entry child_offsets_ptr = NULL (no child_offsets)
    assert_eq!(u64::from_le_bytes(payload[48..56].try_into().unwrap()), 0);
    // [56..64] outer entry field_metadata_ptr = 0xBBBB (LV)
    assert_eq!(
        u64::from_le_bytes(payload[56..64].try_into().unwrap()),
        0xBBBB
    );
    // [64..72] outer entry method_table_ptr = NULL (no methods)
    assert_eq!(u64::from_le_bytes(payload[64..72].try_into().unwrap()), 0);
    // [72..76] count = 1
    assert_eq!(u32::from_le_bytes(payload[72..76].try_into().unwrap()), 1);
}

#[test]
fn fields_with_child_offsets_rebase_target_order() {
    // 2 classes:
    //   class 0: child_offsets=[16], fields=[("a", 24, 1)]
    //   class 1: child_offsets=[],   fields=[("b", 24, 4), ("cd", 32, 6)]
    //
    // Expected rebase targets in order:
    //   Phase 1 (name_ptr): class 0 "a", class 1 "b", class 1 "cd"
    //   Phase 2 (outer slots):
    //     class 0: child_offsets_ptr, field_metadata_ptr
    //     class 1: no child_offsets_ptr (no inner), field_metadata_ptr
    // Total: 3 + 3 = 6.
    let class_layouts = [
        entry_with_fields(&[16], vec![fmeta("a", 24, 1)]),
        entry_with_fields(&[], vec![fmeta("b", 24, 4), fmeta("cd", 32, 6)]),
    ];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    let targets = compute_class_layouts_rebase_targets(&layout, &[], 0x1_0000_0000, 0x1_0000_0000);
    assert_eq!(targets.len(), 6);

    // class 0 has child_offsets [16] (4B), then name "a" (1B), then pad
    // to 8 (3B), then field_meta array (8B header + 24B body).
    // class 1 has no child_offsets; names "b" (1B) + "cd" (2B) = 3B;
    // pad to 8 (5B); field_meta array (8B header + 48B body).
    let class0_fm_array = layout.entries[0].field_meta_array_vaddr.unwrap();
    let class1_fm_array = layout.entries[1].field_meta_array_vaddr.unwrap();
    // Target 0: class 0 "a" name_ptr slot inside class0 array @ +8
    assert_eq!(
        targets[0],
        (
            (class0_fm_array + 8) - 0x1_0000_0000,
            layout.entries[0].field_metadata[0].name_vaddr - 0x1_0000_0000
        )
    );
    // Target 1: class 1 "b" name_ptr slot @ +8 in class1 array
    assert_eq!(
        targets[1],
        (
            (class1_fm_array + 8) - 0x1_0000_0000,
            layout.entries[1].field_metadata[0].name_vaddr - 0x1_0000_0000
        )
    );
    // Target 2: class 1 "cd" name_ptr slot @ +32 (8 header + 24 elem 0)
    assert_eq!(
        targets[2],
        (
            (class1_fm_array + 8 + 24) - 0x1_0000_0000,
            layout.entries[1].field_metadata[1].name_vaddr - 0x1_0000_0000
        )
    );
    // Target 3: class 0 outer child_offsets_ptr @ entry_vaddr + 8
    assert_eq!(
        targets[3],
        (
            (layout.entries[0].entry_vaddr + 8) - 0x1_0000_0000,
            layout.entries[0].inner_vaddr.unwrap() - 0x1_0000_0000
        )
    );
    // Target 4: class 0 outer field_metadata_ptr @ entry_vaddr + 16
    assert_eq!(
        targets[4],
        (
            (layout.entries[0].entry_vaddr + 16) - 0x1_0000_0000,
            class0_fm_array - 0x1_0000_0000
        )
    );
    // Target 5: class 1 outer field_metadata_ptr @ entry_vaddr + 16
    // (no child_offsets_ptr rebase since class 1 inner_vaddr is None)
    assert_eq!(
        targets[5],
        (
            (layout.entries[1].entry_vaddr + 16) - 0x1_0000_0000,
            class1_fm_array - 0x1_0000_0000
        )
    );
}

#[test]
fn round_trip_payload_total_size_matches_layout() {
    // Sanity: feed a mixed shape into compute + payload and verify byte
    // count matches `layout.total_size` exactly.
    let class_layouts = [
        entry_with_fields(&[16, 32], vec![fmeta("x", 24, 1), fmeta("yz", 32, 4)]),
        entry(&[]),
        entry_with_fields(&[], vec![fmeta("alpha", 24, 0)]),
    ];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    // Link value count = total rebase target count.
    let targets = compute_class_layouts_rebase_targets(&layout, &[], 0x1_0000_0000, 0x1_0000_0000);
    let lvs: Vec<u64> = (0..targets.len() as u64).map(|i| 0xC000 + i).collect();
    let payload = build_user_class_layouts_payload(&layout, &lvs);
    assert_eq!(payload.len() as u32, layout.total_size);
}

// Helper trait so the layout-shape test can sanity-check the name
// string's file_offset without exposing the field name across modules
// (the placement struct's name_file_offset is `pub` already, this just
// keeps the test assertion explicit).
trait FieldMetaPlacementExt {
    fn name_byte_offset_check(&self) -> u32;
}

impl FieldMetaPlacementExt for super::types::UserFieldMetaPlacement {
    fn name_byte_offset_check(&self) -> u32 {
        self.name_file_offset
    }
}

// ---------------- 刀 4 — class-methods table tests ----------------

pub(super) fn entry_with_methods(
    offsets: &[u32],
    methods: Vec<crate::exec::UserMethodMetaEntry>,
) -> UserClassLayoutEntry {
    UserClassLayoutEntry {
        child_offsets: offsets.to_vec(),
        fields: Vec::new(),
        is_named: true,
        methods,
    }
}

pub(super) fn mmeta(name: &str, adapter_fn_id: u32) -> crate::exec::UserMethodMetaEntry {
    crate::exec::UserMethodMetaEntry {
        name: name.into(),
        adapter_fn_id,
        flags: 0,
        twin_fn_id: None,
    }
}

#[test]
fn single_entry_with_one_method_layout_and_payload() {
    // 1 class, no child_offsets, no fields, 1 method "next" (adapter
    // fn id 7, no twin). Inner: name "next" (4B) + pad to 8 (4B) +
    // method_meta header (8B) + 1 MethodMeta (32B) = 48B. Outer:
    // 32B. Count: 4B.
    let class_layouts = [entry_with_methods(&[], vec![mmeta("next", 7)])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    let e = &layout.entries[0];
    assert!(e.inner_vaddr.is_none());
    assert!(e.field_meta_array_vaddr.is_none());
    assert_eq!(e.methods.len(), 1);
    assert_eq!(e.methods[0].name_bytes, b"next");
    assert_eq!(e.methods[0].adapter_fn_id, 7);
    assert_eq!(e.method_meta_array_file_offset, 0x4008);
    assert_eq!(e.method_meta_array_vaddr, Some(0x1_0000_4008));
    assert_eq!(layout.total_size, 48 + 32 + 4);

    // Rebase order: method name_ptr, adapter_ptr (no twin target),
    // outer method_table_ptr.
    let fn_vaddrs = vec![0u64; 8];
    let mut fv = fn_vaddrs.clone();
    fv[7] = 0x1_0000_9000;
    let targets = compute_class_layouts_rebase_targets(&layout, &fv, 0x1_0000_0000, 0x1_0000_0000);
    assert_eq!(targets.len(), 3);
    let array = e.method_meta_array_vaddr.unwrap();
    // name_ptr slot @ array + 8 (header) + 0.
    assert_eq!(
        targets[0],
        (
            (array + 8) - 0x1_0000_0000,
            e.methods[0].name_vaddr - 0x1_0000_0000
        )
    );
    // adapter_ptr slot @ array + 8 + 16.
    assert_eq!(
        targets[1],
        (
            (array + 8 + 16) - 0x1_0000_0000,
            0x1_0000_9000 - 0x1_0000_0000
        )
    );
    // outer method_table_ptr @ entry + 24.
    assert_eq!(
        targets[2],
        ((e.entry_vaddr + 24) - 0x1_0000_0000, array - 0x1_0000_0000)
    );

    // Payload: 3 link values in the same order.
    let lvs = [0x1111u64, 0x2222u64, 0x3333u64];
    let payload = build_user_class_layouts_payload(&layout, &lvs);
    assert_eq!(payload.len() as u32, layout.total_size);
    // [0..4] name "next"; [4..8] pad.
    assert_eq!(&payload[0..4], b"next");
    // [8..12] header n_methods = 1.
    assert_eq!(u32::from_le_bytes(payload[8..12].try_into().unwrap()), 1);
    // [16..24] MethodMeta name_ptr = 0x1111.
    assert_eq!(
        u64::from_le_bytes(payload[16..24].try_into().unwrap()),
        0x1111
    );
    // [24..28] name_len = 4.
    assert_eq!(u32::from_le_bytes(payload[24..28].try_into().unwrap()), 4);
    // [32..40] adapter_ptr = 0x2222.
    assert_eq!(
        u64::from_le_bytes(payload[32..40].try_into().unwrap()),
        0x2222
    );
    // [40..48] twin_ptr — no twin, raw 0 (consumes no link value).
    assert_eq!(u64::from_le_bytes(payload[40..48].try_into().unwrap()), 0);
    // Outer entry starts at 48: method_table_ptr @ +24 = 0x3333.
    assert_eq!(
        u64::from_le_bytes(payload[72..80].try_into().unwrap()),
        0x3333
    );
    // Count @ 80.
    assert_eq!(u32::from_le_bytes(payload[80..84].try_into().unwrap()), 1);
}
