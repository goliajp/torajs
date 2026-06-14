//! Tests for the class-layouts region — exercise via the public API
//! re-exported from the parent module.

use super::*;
use crate::exec::UserClassLayoutEntry;
use crate::resolve::SymTable;

fn entry(offsets: &[u32]) -> UserClassLayoutEntry {
    UserClassLayoutEntry {
        child_offsets: offsets.to_vec(),
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
    // tr build path — staticlib references the syms even when the
    // program has no classes; force_emit_globals lands a 4-byte u32
    // count = 0 + aliases both sym vaddrs to it.
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
fn single_entry_two_offsets() {
    // 1 entry, 2 child offsets → inner 8 bytes + outer 24 bytes +
    // count 4 bytes = 36 bytes (W-J Phase A2: was 28 pre-A2 when
    // OUTER_ENTRY_SIZE was 16). cursor 0x4000 is 8-aligned so no
    // leading pads needed.
    let class_layouts = [entry(&[24, 32])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    assert_eq!(layout.entries.len(), 1);
    // Inner starts at base.
    assert_eq!(layout.entries[0].inner_file_offset, 0x4000);
    assert_eq!(layout.entries[0].inner_vaddr, Some(0x1_0000_4000));
    // Outer past 8-byte inner.
    assert_eq!(layout.outer_file_offset, 0x4008);
    assert_eq!(layout.outer_vaddr, 0x1_0000_4008);
    // Count past 24-byte outer.
    assert_eq!(layout.count_file_offset, 0x4020);
    assert_eq!(layout.count_vaddr, 0x1_0000_4020);
    // Total = 8 + 24 + 4 = 36.
    assert_eq!(layout.total_size, 36);
}

#[test]
fn empty_child_offsets_skips_inner_global() {
    // 1 entry, 0 child offsets → no inner; outer 24 bytes (n=0,
    // ptr=0, field_meta=0) + count 4 bytes = 28 bytes (W-J Phase A2:
    // was 20 pre-A2 with 16B entry).
    let class_layouts = [entry(&[])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    assert!(layout.entries[0].inner_vaddr.is_none());
    assert_eq!(layout.outer_file_offset, 0x4000);
    assert_eq!(layout.total_size, 28);
}

#[test]
fn mixed_empty_and_nonempty_pack_back_to_back() {
    // Entry 0: 1 offset (inner 4 bytes + pad 4 to outer 8-align)
    // Entry 1: empty (no inner)
    // Entry 2: 2 offsets (inner 8 bytes back-to-back after entry 0's 4)
    let class_layouts = [entry(&[16]), entry(&[]), entry(&[32, 40])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    assert_eq!(layout.entries[0].inner_file_offset, 0x4000);
    assert_eq!(layout.entries[0].inner_vaddr, Some(0x1_0000_4000));
    assert!(layout.entries[1].inner_vaddr.is_none());
    // Entry 2 inner sits at 0x4004 (right after entry 0's 4 bytes).
    assert_eq!(layout.entries[2].inner_file_offset, 0x4004);
    // Total inner = 4 + 8 = 12 bytes; outer 8-align pad = 4 bytes.
    // Outer starts at 0x4010.
    assert_eq!(layout.outer_file_offset, 0x4010);
    // W-J Phase A2: 3 outer entries × 24 = 72 bytes; count at 0x4058.
    assert_eq!(layout.count_file_offset, 0x4058);
    assert_eq!(layout.total_size, 4 + 8 + 4 + 72 + 4); // 92
}

#[test]
fn build_payload_emits_inner_outer_count_in_order() {
    let class_layouts = [entry(&[16, 24])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    let link_values = [0xDEAD_BEEFu64];
    let payload = build_user_class_layouts_payload(&layout, &link_values);
    // W-J Phase A2: 8 inner + 24 outer + 4 count = 36 (was 28).
    assert_eq!(payload.len(), 36);
    // [0..8]   inner [16, 24] LE
    assert_eq!(u32::from_le_bytes(payload[0..4].try_into().unwrap()), 16);
    assert_eq!(u32::from_le_bytes(payload[4..8].try_into().unwrap()), 24);
    // [8..12]  outer entry n_children = 2
    assert_eq!(u32::from_le_bytes(payload[8..12].try_into().unwrap()), 2);
    // [12..16] outer entry pad = 0
    assert_eq!(u32::from_le_bytes(payload[12..16].try_into().unwrap()), 0);
    // [16..24] outer entry inner-ptr = link value
    assert_eq!(
        u64::from_le_bytes(payload[16..24].try_into().unwrap()),
        0xDEAD_BEEF
    );
    // W-J Phase A2: [24..32] outer entry field_metadata_ptr = NULL stub
    assert_eq!(u64::from_le_bytes(payload[24..32].try_into().unwrap()), 0);
    // [32..36] count = 1
    assert_eq!(u32::from_le_bytes(payload[32..36].try_into().unwrap()), 1);
}

#[test]
fn build_payload_zeroes_empty_entry_outer_slot() {
    let class_layouts = [entry(&[])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    let payload = build_user_class_layouts_payload(&layout, &[]);
    // W-J Phase A2: 24 outer + 4 count = 28 (was 20).
    assert_eq!(payload.len(), 28);
    // outer entry n_children = 0
    assert_eq!(u32::from_le_bytes(payload[0..4].try_into().unwrap()), 0);
    // outer entry inner-ptr = NULL
    assert_eq!(u64::from_le_bytes(payload[8..16].try_into().unwrap()), 0);
    // W-J Phase A2: field_metadata_ptr NULL stub
    assert_eq!(u64::from_le_bytes(payload[16..24].try_into().unwrap()), 0);
    // count = 1
    assert_eq!(u32::from_le_bytes(payload[24..28].try_into().unwrap()), 1);
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
fn rebase_targets_emit_one_per_nonempty_entry() {
    let class_layouts = [entry(&[16]), entry(&[]), entry(&[24, 32])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    let targets = compute_class_layouts_rebase_targets(&layout, 0x1_0000_0000, 0x1_0000_0000);
    // 2 non-empty entries → 2 rebase targets.
    assert_eq!(targets.len(), 2);
    // Entry 0 slot vaddr = entry_vaddr + 8 (ptr offset), target =
    // entry 0's inner_vaddr.
    let entry0_slot_off = (layout.entries[0].entry_vaddr - 0x1_0000_0000) + 8;
    let entry0_target_off = layout.entries[0].inner_vaddr.unwrap() - 0x1_0000_0000;
    assert_eq!(targets[0], (entry0_slot_off, entry0_target_off));
    // Entry 2 (skipping empty entry 1).
    let entry2_slot_off = (layout.entries[2].entry_vaddr - 0x1_0000_0000) + 8;
    let entry2_target_off = layout.entries[2].inner_vaddr.unwrap() - 0x1_0000_0000;
    assert_eq!(targets[1], (entry2_slot_off, entry2_target_off));
}
