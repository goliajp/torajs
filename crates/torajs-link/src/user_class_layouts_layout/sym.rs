//! Sym-table override + chain-fixup rebase target derivation +
//! extra-defined-syms set for the class-layouts region.

use super::types::{
    CLASS_LAYOUTS_SYM, N_CLASS_LAYOUTS_SYM, OUTER_PTR_OFFSET_IN_ENTRY, UserClassLayoutsLayout,
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

/// Derive the rebase target list for the outer-table inner-ptr slots
/// — one `RebaseTarget` per entry whose `inner_vaddr` is `Some(_)`.
/// `seg_vmaddr_base` = `__DATA_CONST` segment base; `image_vmaddr_base`
/// = `TEXT_VMADDR_BASE`. Same conventions as `compute_vtable_rebase_targets`.
pub fn compute_class_layouts_rebase_targets(
    layout: &UserClassLayoutsLayout,
    seg_vmaddr_base: u64,
    image_vmaddr_base: u64,
) -> Vec<RebaseTarget> {
    let mut targets = Vec::new();
    for entry in &layout.entries {
        let Some(inner_vaddr) = entry.inner_vaddr else {
            continue;
        };
        let slot_vaddr = entry.entry_vaddr + u64::from(OUTER_PTR_OFFSET_IN_ENTRY);
        debug_assert!(
            slot_vaddr >= seg_vmaddr_base,
            "class_layouts slot vaddr {slot_vaddr:#x} cannot precede segment base {seg_vmaddr_base:#x}",
        );
        debug_assert!(
            inner_vaddr >= image_vmaddr_base,
            "class_layouts inner_vaddr {inner_vaddr:#x} cannot precede image base {image_vmaddr_base:#x}",
        );
        let slot_off_in_seg = slot_vaddr - seg_vmaddr_base;
        let target_off_in_image = inner_vaddr - image_vmaddr_base;
        targets.push((slot_off_in_seg, target_off_in_image));
    }
    targets
}
