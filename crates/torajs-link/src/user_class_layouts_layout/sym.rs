//! Sym-table override + chain-fixup rebase target derivation +
//! extra-defined-syms set for the class-layouts region.

use super::types::{
    CLASS_LAYOUTS_SYM, FIELD_META_NAME_PTR_OFFSET_IN_ELEM, INNER_FIELD_META_ELEM_SIZE,
    INNER_FIELD_META_HEADER_SIZE, INNER_METHOD_META_ELEM_SIZE, INNER_METHOD_META_HEADER_SIZE,
    METHOD_META_ADAPTER_PTR_OFFSET_IN_ELEM, METHOD_META_NAME_PTR_OFFSET_IN_ELEM,
    METHOD_META_TWIN_PTR_OFFSET_IN_ELEM, N_CLASS_LAYOUTS_SYM,
    OUTER_CHILD_OFFSETS_PTR_OFFSET_IN_ENTRY, OUTER_FIELD_META_PTR_OFFSET_IN_ENTRY,
    OUTER_METHOD_TABLE_PTR_OFFSET_IN_ENTRY, UserClassLayoutsLayout,
};
use crate::chained_fixups_starts::RebaseTarget;
use crate::exec::UserClassLayoutEntry;
use crate::resolve::SymTable;

/// Register `__torajs_class_layouts` + `__torajs_n_class_layouts` →
/// vaddr into the effective sym table.
pub fn apply_user_class_layouts_overrides(
    layout: &UserClassLayoutsLayout,
    sym_table: &mut SymTable,
) {
    if layout.total_size == 0 {
        return;
    }
    sym_table.insert(CLASS_LAYOUTS_SYM.into(), layout.outer_vaddr);
    sym_table.insert(N_CLASS_LAYOUTS_SYM.into(), layout.count_vaddr);
}

/// Sym names to flag as link-defined so the worklist closure doesn't
/// report them as unresolved externs.
pub fn user_class_layouts_extra_defined_syms(
    class_layouts: &[UserClassLayoutEntry],
    force_emit_globals: bool,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    if !class_layouts.is_empty() || force_emit_globals {
        out.insert(CLASS_LAYOUTS_SYM.into());
        out.insert(N_CLASS_LAYOUTS_SYM.into());
    }
    out
}

/// Derive the chain-fixup rebase target list for the class-layouts
/// region. Order MUST stay in lockstep with
/// `build_user_class_layouts_payload`'s link-value consumption:
///   Phase 1 — per class, in entry-major order: the FieldMeta
///             `name_ptr` slots (one per field), then 刀 4's
///             MethodMeta `name_ptr` + `adapter_ptr` slot pairs
///             (name then adapter, per method). The adapter target
///             is `fn_vaddrs[adapter_fn_id]` (the vtable slots'
///             fn-vaddr mechanism).
///   Phase 2 — per-class outer-entry slots, in entry-major order: at
///             most one `child_offsets_ptr`, one `field_metadata_ptr`
///             and one `method_table_ptr` per entry (skipped when the
///             inner global is absent).
pub fn compute_class_layouts_rebase_targets(
    layout: &UserClassLayoutsLayout,
    fn_vaddrs: &[u64],
    seg_vmaddr_base: u64,
    image_vmaddr_base: u64,
) -> Vec<RebaseTarget> {
    let mut targets = Vec::new();
    // Phase 1: per entry — FieldMeta.name_ptr slots, then MethodMeta
    // name_ptr + adapter_ptr pairs (compute's inner-cursor order).
    for entry in &layout.entries {
        if let Some(array_vaddr) = entry.field_meta_array_vaddr {
            for (j, fm) in entry.field_metadata.iter().enumerate() {
                let slot_vaddr = array_vaddr
                    + u64::from(INNER_FIELD_META_HEADER_SIZE)
                    + (j as u64) * u64::from(INNER_FIELD_META_ELEM_SIZE)
                    + u64::from(FIELD_META_NAME_PTR_OFFSET_IN_ELEM);
                debug_assert!(
                    slot_vaddr >= seg_vmaddr_base,
                    "class_layouts FieldMeta name_ptr slot vaddr {slot_vaddr:#x} cannot precede segment base {seg_vmaddr_base:#x}",
                );
                debug_assert!(
                    fm.name_vaddr >= image_vmaddr_base,
                    "class_layouts FieldMeta name target {:#x} cannot precede image base {image_vmaddr_base:#x}",
                    fm.name_vaddr,
                );
                targets.push((
                    slot_vaddr - seg_vmaddr_base,
                    fm.name_vaddr - image_vmaddr_base,
                ));
            }
        }
        if let Some(array_vaddr) = entry.method_meta_array_vaddr {
            for (j, mm) in entry.methods.iter().enumerate() {
                let elem_vaddr = array_vaddr
                    + u64::from(INNER_METHOD_META_HEADER_SIZE)
                    + (j as u64) * u64::from(INNER_METHOD_META_ELEM_SIZE);
                let name_slot = elem_vaddr + u64::from(METHOD_META_NAME_PTR_OFFSET_IN_ELEM);
                targets.push((
                    name_slot - seg_vmaddr_base,
                    mm.name_vaddr - image_vmaddr_base,
                ));
                // r502 — the adapter slot, only while the link keeps
                // the column (payload writes a raw 0 otherwise;
                // lockstep with build_user_class_layouts_payload).
                if let Some(adapter_id) = mm.adapter_fn_id {
                    let adapter_vaddr = fn_vaddrs[adapter_id as usize];
                    debug_assert!(
                        adapter_vaddr >= image_vmaddr_base,
                        "class_methods adapter target {adapter_vaddr:#x} cannot precede image base {image_vmaddr_base:#x}",
                    );
                    let adapter_slot =
                        elem_vaddr + u64::from(METHOD_META_ADAPTER_PTR_OFFSET_IN_ELEM);
                    targets.push((
                        adapter_slot - seg_vmaddr_base,
                        adapter_vaddr - image_vmaddr_base,
                    ));
                }
                // Blade 3 — the twin adapter slot, only when minted
                // (payload writes a raw 0 otherwise; lockstep with
                // build_user_class_layouts_payload's take order).
                if let Some(twin_id) = mm.twin_fn_id {
                    let twin_vaddr = fn_vaddrs[twin_id as usize];
                    debug_assert!(
                        twin_vaddr >= image_vmaddr_base,
                        "class_methods twin target {twin_vaddr:#x} cannot precede image base {image_vmaddr_base:#x}",
                    );
                    let twin_slot = elem_vaddr + u64::from(METHOD_META_TWIN_PTR_OFFSET_IN_ELEM);
                    targets.push((twin_slot - seg_vmaddr_base, twin_vaddr - image_vmaddr_base));
                }
            }
        }
    }
    // Phase 2: per-class outer-entry slots.
    for entry in &layout.entries {
        if let Some(inner_vaddr) = entry.inner_vaddr {
            let slot_vaddr = entry.entry_vaddr + u64::from(OUTER_CHILD_OFFSETS_PTR_OFFSET_IN_ENTRY);
            debug_assert!(
                slot_vaddr >= seg_vmaddr_base,
                "class_layouts child_offsets_ptr slot vaddr {slot_vaddr:#x} cannot precede segment base {seg_vmaddr_base:#x}",
            );
            debug_assert!(
                inner_vaddr >= image_vmaddr_base,
                "class_layouts child_offsets target {inner_vaddr:#x} cannot precede image base {image_vmaddr_base:#x}",
            );
            targets.push((
                slot_vaddr - seg_vmaddr_base,
                inner_vaddr - image_vmaddr_base,
            ));
        }
        if let Some(array_vaddr) = entry.field_meta_array_vaddr {
            let slot_vaddr = entry.entry_vaddr + u64::from(OUTER_FIELD_META_PTR_OFFSET_IN_ENTRY);
            debug_assert!(
                slot_vaddr >= seg_vmaddr_base,
                "class_layouts field_metadata_ptr slot vaddr {slot_vaddr:#x} cannot precede segment base {seg_vmaddr_base:#x}",
            );
            debug_assert!(
                array_vaddr >= image_vmaddr_base,
                "class_layouts field_meta_array target {array_vaddr:#x} cannot precede image base {image_vmaddr_base:#x}",
            );
            targets.push((
                slot_vaddr - seg_vmaddr_base,
                array_vaddr - image_vmaddr_base,
            ));
        }
        // 刀 4 — method_table_ptr slot.
        if let Some(array_vaddr) = entry.method_meta_array_vaddr {
            let slot_vaddr = entry.entry_vaddr + u64::from(OUTER_METHOD_TABLE_PTR_OFFSET_IN_ENTRY);
            targets.push((
                slot_vaddr - seg_vmaddr_base,
                array_vaddr - image_vmaddr_base,
            ));
        }
    }
    targets
}
