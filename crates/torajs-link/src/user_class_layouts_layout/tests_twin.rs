//! Blade-3 twin-slot tests — split out of `tests.rs` (file-size
//! line, rotation 296): the MethodMeta twin_ptr's rebase-target /
//! link-value lockstep coverage.

use super::tests::{entry_with_methods, mmeta};
use super::{
    build_user_class_layouts_payload, compute_class_layouts_rebase_targets,
    compute_user_class_layouts_layout,
};

#[test]
fn method_with_twin_takes_fourth_link_value() {
    // Blade 3 — a minted twin (fn id 5) adds one rebase target after
    // the adapter and its payload slot consumes the extra link value.
    let mut mm = mmeta("next", 7);
    mm.twin_fn_id = Some(5);
    let class_layouts = [entry_with_methods(&[], vec![mm])];
    let layout = compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
    let e = &layout.entries[0];
    let mut fv = vec![0u64; 8];
    fv[7] = 0x1_0000_9000;
    fv[5] = 0x1_0000_A000;
    let targets = compute_class_layouts_rebase_targets(&layout, &fv, 0x1_0000_0000, 0x1_0000_0000);
    assert_eq!(targets.len(), 4);
    let array = e.method_meta_array_vaddr.unwrap();
    // twin slot @ array + 8 + 24, between adapter and the outer ptr.
    assert_eq!(
        targets[2],
        (
            (array + 8 + 24) - 0x1_0000_0000,
            0x1_0000_A000 - 0x1_0000_0000
        )
    );
    let lvs = [0x1111u64, 0x2222u64, 0x4444u64, 0x3333u64];
    let payload = build_user_class_layouts_payload(&layout, &lvs);
    // [40..48] twin_ptr = the third link value.
    assert_eq!(
        u64::from_le_bytes(payload[40..48].try_into().unwrap()),
        0x4444
    );
    // Outer method_table_ptr takes the fourth.
    assert_eq!(
        u64::from_le_bytes(payload[72..80].try_into().unwrap()),
        0x3333
    );
}
